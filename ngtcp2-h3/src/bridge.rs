use std::{
    collections::{HashMap, VecDeque},
    convert::TryInto,
    sync::{Arc, Mutex, MutexGuard},
    task::{Context, Poll},
};

use bytes::{Buf, Bytes};
use h3::{
    error::Code,
    quic::{self, ConnectionErrorIncoming, StreamErrorIncoming, StreamId as H3StreamId, WriteBuf},
};
use ngnet_quic::{
    ApplicationErrorCode, CloseReason, Directionality, ErrorKind, ExpiryOutcome, Initiator,
    ReadOutcome, Role, Session, StreamId, StreamWrite, WriteOutcome,
    endpoint::{DetachedConnection, Observed, Sleep},
};

const MAX_DATAGRAM: usize = 1500;
const MAX_DATAGRAMS_PER_POLL: usize = 64;

#[derive(Default)]
struct ReceiveState {
    chunks: VecDeque<Bytes>,
    finished: bool,
    reset: Option<u64>,
}

struct Inner<S: Session> {
    detached: DetachedConnection<S>,
    local: Initiator,
    closed: Option<ConnectionErrorIncoming>,
    incoming_bidi: VecDeque<StreamId>,
    incoming_uni: VecDeque<StreamId>,
    receive: HashMap<i64, ReceiveState>,
    stopped: HashMap<i64, u64>,
    sleeping: Option<Sleep>,
    sleeping_until: Option<ngnet_quic::Timestamp>,
    scratch: Vec<u8>,
}

impl<S: Session> Inner<S> {
    fn new(detached: DetachedConnection<S>, role: Role) -> Self {
        Self {
            detached,
            local: match role {
                Role::Client => Initiator::Client,
                Role::Server => Initiator::Server,
            },
            closed: None,
            incoming_bidi: VecDeque::new(),
            incoming_uni: VecDeque::new(),
            receive: HashMap::new(),
            stopped: HashMap::new(),
            sleeping: None,
            sleeping_until: None,
            scratch: vec![0; MAX_DATAGRAM],
        }
    }

    fn connection_error(&self) -> ConnectionErrorIncoming {
        self.closed.clone().unwrap_or_else(|| {
            ConnectionErrorIncoming::Undefined(Arc::new(BridgeError(
                "the QUIC connection is closed",
            )))
        })
    }

    fn mark_transport_error(&mut self, error: ngnet_quic::Error) -> ConnectionErrorIncoming {
        let error = ConnectionErrorIncoming::Undefined(Arc::new(error));
        self.closed = Some(error.clone());
        self.detached.release();
        error
    }

    fn mark_closed(&mut self, timeout: bool) {
        if self.closed.is_some() {
            return;
        }
        self.closed = Some(if timeout {
            ConnectionErrorIncoming::Timeout
        } else {
            match self.detached.conn.close_error().reason() {
                CloseReason::Application(code) => ConnectionErrorIncoming::ApplicationClose {
                    error_code: code.get(),
                },
                CloseReason::IdleTimeout => ConnectionErrorIncoming::Timeout,
                _ => ConnectionErrorIncoming::Undefined(Arc::new(BridgeError(
                    "the QUIC transport closed the connection",
                ))),
            }
        });
        self.detached.release();
    }

    fn collect(&mut self) {
        for observed in self.detached.take_observed() {
            match observed {
                Observed::Data(id, bytes, fin) => {
                    let state = self.receive.entry(id.get()).or_default();
                    if !bytes.is_empty() {
                        state.chunks.push_back(Bytes::from(bytes));
                    }
                    state.finished |= fin;
                }
                Observed::Opened(id) => match id.directionality() {
                    Directionality::Bidirectional => self.incoming_bidi.push_back(id),
                    Directionality::Unidirectional => self.incoming_uni.push_back(id),
                },
                Observed::Closed(id, reason) => {
                    if id.initiator() != self.local {
                        match id.directionality() {
                            Directionality::Bidirectional => {
                                self.detached.conn.extend_max_streams_bidi(1);
                            }
                            Directionality::Unidirectional => {
                                self.detached.conn.extend_max_streams_uni(1);
                            }
                        }
                    }
                    let state = self.receive.entry(id.get()).or_default();
                    state.finished = true;
                    if let Some(code) = reason.receiving() {
                        state.reset.get_or_insert(code.get());
                    }
                    if let Some(code) = reason.sending() {
                        self.stopped.entry(id.get()).or_insert(code.get());
                    }
                }
                Observed::Reset(id, code) => {
                    let state = self.receive.entry(id.get()).or_default();
                    state.finished = true;
                    state.reset.get_or_insert(code.get());
                }
                Observed::StopSending(id, code) => {
                    self.stopped.entry(id.get()).or_insert(code.get());
                }
                Observed::LocallyOpened(_)
                | Observed::Acked(..)
                | Observed::HandshakeCompleted
                | Observed::StreamsExtended(_) => {}
                _ => {}
            }
        }
    }

    fn drive(&mut self, cx: &mut Context<'_>) -> Result<(), ConnectionErrorIncoming> {
        self.detached.register(cx.waker());
        if self.closed.is_some() {
            return Err(self.connection_error());
        }

        let now = self.detached.now();
        while let Some(datagram) = self.detached.next_inbound() {
            match self.detached.conn.read_pkt(&datagram, now) {
                Ok(ReadOutcome::Processed | ReadOutcome::SendRetry | ReadOutcome::DropSilently) => {
                }
                Ok(ReadOutcome::Draining | ReadOutcome::Closing) => {
                    self.mark_closed(false);
                    break;
                }
                Err(error) => return Err(self.mark_transport_error(error)),
            }
        }
        self.collect();

        if self.closed.is_some() {
            return Err(self.connection_error());
        }

        if self
            .detached
            .conn
            .expiry()
            .is_some_and(|expiry| expiry <= now)
        {
            match self.detached.conn.handle_expiry(now) {
                Ok(ExpiryOutcome::Handled) => {}
                Ok(ExpiryOutcome::IdleClose) => self.mark_closed(true),
                Ok(ExpiryOutcome::Terminal) => self.mark_closed(false),
                Err(error) => return Err(self.mark_transport_error(error)),
            }
            self.collect();
        }

        if self.closed.is_some() {
            return Err(self.connection_error());
        }

        self.produce()?;
        self.poll_timer(cx);
        Ok(())
    }

    fn produce(&mut self) -> Result<(), ConnectionErrorIncoming> {
        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            if !self.detached.outbound_has_room() {
                break;
            }
            let now = self.detached.now();
            let mut datagram = std::mem::take(&mut self.scratch);
            datagram.resize(MAX_DATAGRAM, 0);
            match self.detached.conn.write_pkt(&mut datagram, now) {
                Ok(WriteOutcome::Datagram { len }) => {
                    datagram.truncate(len);
                    self.detached.send(datagram);
                }
                Ok(WriteOutcome::Blocked | WriteOutcome::Idle) => {
                    datagram.clear();
                    self.scratch = datagram;
                    break;
                }
                Err(error) => {
                    self.scratch = datagram;
                    return Err(self.mark_transport_error(error));
                }
            }
        }
        Ok(())
    }

    fn poll_timer(&mut self, cx: &mut Context<'_>) {
        let Some(deadline) = self.detached.conn.expiry() else {
            self.sleeping = None;
            self.sleeping_until = None;
            return;
        };
        if self.sleeping_until != Some(deadline) {
            self.sleeping = Some(self.detached.sleep_until(deadline));
            self.sleeping_until = Some(deadline);
        }
        if self
            .sleeping
            .as_mut()
            .is_some_and(|sleep| sleep.as_mut().poll(cx).is_ready())
        {
            self.sleeping = None;
            self.sleeping_until = None;
            cx.waker().wake_by_ref();
        }
    }

    fn send_chunk(
        &mut self,
        stream: StreamId,
        data: &[u8],
        fin: bool,
    ) -> Result<StreamWrite, StreamErrorIncoming> {
        if let Some(code) = self.stopped.get(&stream.get()) {
            return Err(StreamErrorIncoming::StreamTerminated { error_code: *code });
        }
        if self.closed.is_some() {
            return Err(StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: self.connection_error(),
            });
        }
        if !self.detached.outbound_has_room() {
            return Ok(StreamWrite::Blocked);
        }

        let now = self.detached.now();
        let mut datagram = std::mem::take(&mut self.scratch);
        datagram.resize(MAX_DATAGRAM, 0);
        let outcome = self
            .detached
            .conn
            .write_stream(&mut datagram, stream, data, fin, now)
            .map_err(stream_error)?;
        if let StreamWrite::Datagram { len, .. } = outcome {
            datagram.truncate(len);
            self.detached.send(datagram);
        } else {
            datagram.clear();
            self.scratch = datagram;
        }
        Ok(outcome)
    }

    fn close(&mut self, code: Code, reason: &[u8]) {
        if self.closed.is_some() {
            return;
        }
        let now = self.detached.now();
        let mut datagram = std::mem::take(&mut self.scratch);
        datagram.resize(MAX_DATAGRAM, 0);
        if let Ok(len) = self.detached.conn.write_connection_close(
            &mut datagram,
            ApplicationErrorCode::new(code.value()),
            reason,
            now,
        ) && len > 0
        {
            datagram.truncate(len);
            self.detached.send(datagram);
        }
        self.closed = Some(ConnectionErrorIncoming::Undefined(Arc::new(BridgeError(
            "the QUIC connection was closed locally",
        ))));
        self.detached.release();
    }
}

impl<S: Session> Drop for Inner<S> {
    fn drop(&mut self) {
        self.detached.release();
    }
}

struct Shared<S: Session>(Mutex<Inner<S>>);

impl<S: Session> Shared<S> {
    fn lock(&self) -> MutexGuard<'_, Inner<S>> {
        self.0
            .lock()
            .unwrap_or_else(|poisoned| poisoned.into_inner())
    }
}

/// An established ngtcp2 connection implementing the `h3` transport traits.
pub struct Ngtcp2Connection<S: Session> {
    shared: Arc<Shared<S>>,
}

impl<S: Session> Ngtcp2Connection<S> {
    /// Wraps a detached client connection.
    pub fn client(detached: DetachedConnection<S>) -> Self {
        Self::new(detached, Role::Client)
    }

    /// Wraps a detached server connection.
    pub fn server(detached: DetachedConnection<S>) -> Self {
        Self::new(detached, Role::Server)
    }

    fn new(detached: DetachedConnection<S>, role: Role) -> Self {
        Self {
            shared: Arc::new(Shared(Mutex::new(Inner::new(detached, role)))),
        }
    }

    /// Returns the peer address.
    pub fn remote_address(&self) -> std::net::SocketAddr {
        self.shared.lock().detached.remote
    }

    /// Returns the number of inbound datagrams dropped by the endpoint.
    pub fn dropped_inbound(&self) -> u64 {
        self.shared.lock().detached.dropped_inbound()
    }
}

impl<S: Session> quic::Connection<Bytes> for Ngtcp2Connection<S> {
    type RecvStream = RecvStream<S>;
    type OpenStreams = OpenStreams<S>;

    fn poll_accept_recv(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::RecvStream, ConnectionErrorIncoming>> {
        let mut inner = self.shared.lock();
        let driven = inner.drive(cx);
        if let Some(stream) = inner.incoming_uni.pop_front() {
            return Poll::Ready(Ok(RecvStream::new(Arc::clone(&self.shared), stream)));
        }
        driven?;
        Poll::Pending
    }

    fn poll_accept_bidi(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::BidiStream, ConnectionErrorIncoming>> {
        let mut inner = self.shared.lock();
        let driven = inner.drive(cx);
        if let Some(stream) = inner.incoming_bidi.pop_front() {
            return Poll::Ready(Ok(BidiStream::new(Arc::clone(&self.shared), stream)));
        }
        driven?;
        Poll::Pending
    }

    fn opener(&self) -> Self::OpenStreams {
        OpenStreams {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<S: Session> quic::OpenStreams<Bytes> for Ngtcp2Connection<S> {
    type BidiStream = BidiStream<S>;
    type SendStream = SendStream<S>;

    fn poll_open_bidi(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        poll_open(&self.shared, cx, true)
            .map_ok(|stream| BidiStream::new(Arc::clone(&self.shared), stream))
    }

    fn poll_open_send(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        poll_open(&self.shared, cx, false)
            .map_ok(|stream| SendStream::new(Arc::clone(&self.shared), stream))
    }

    fn close(&mut self, code: Code, reason: &[u8]) {
        self.shared.lock().close(code, reason);
    }
}

/// A cloneable handle for opening streams on an ngtcp2 connection.
pub struct OpenStreams<S: Session> {
    shared: Arc<Shared<S>>,
}

impl<S: Session> Clone for OpenStreams<S> {
    fn clone(&self) -> Self {
        Self {
            shared: Arc::clone(&self.shared),
        }
    }
}

impl<S: Session> quic::OpenStreams<Bytes> for OpenStreams<S> {
    type BidiStream = BidiStream<S>;
    type SendStream = SendStream<S>;

    fn poll_open_bidi(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::BidiStream, StreamErrorIncoming>> {
        poll_open(&self.shared, cx, true)
            .map_ok(|stream| BidiStream::new(Arc::clone(&self.shared), stream))
    }

    fn poll_open_send(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Self::SendStream, StreamErrorIncoming>> {
        poll_open(&self.shared, cx, false)
            .map_ok(|stream| SendStream::new(Arc::clone(&self.shared), stream))
    }

    fn close(&mut self, code: Code, reason: &[u8]) {
        self.shared.lock().close(code, reason);
    }
}

fn poll_open<S: Session>(
    shared: &Arc<Shared<S>>,
    cx: &mut Context<'_>,
    bidi: bool,
) -> Poll<Result<StreamId, StreamErrorIncoming>> {
    let mut inner = shared.lock();
    inner.drive(cx).map_err(
        |connection_error| StreamErrorIncoming::ConnectionErrorIncoming { connection_error },
    )?;
    let opened = if bidi {
        inner.detached.conn.open_bidi_stream()
    } else {
        inner.detached.conn.open_uni_stream()
    };
    match opened {
        Ok(stream) => Poll::Ready(Ok(stream)),
        Err(error) if error.kind() == ErrorKind::Blocked => Poll::Pending,
        Err(error) => Poll::Ready(Err(stream_error(error))),
    }
}

/// The sending half of an ngtcp2 stream.
pub struct SendStream<S: Session> {
    shared: Arc<Shared<S>>,
    stream: StreamId,
    writing: Option<WriteBuf<Bytes>>,
    finished: bool,
}

impl<S: Session> SendStream<S> {
    fn new(shared: Arc<Shared<S>>, stream: StreamId) -> Self {
        Self {
            shared,
            stream,
            writing: None,
            finished: false,
        }
    }

    fn poll_write(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        let mut inner = self.shared.lock();
        inner.drive(cx).map_err(|connection_error| {
            StreamErrorIncoming::ConnectionErrorIncoming { connection_error }
        })?;

        for _ in 0..MAX_DATAGRAMS_PER_POLL {
            let Some(writing) = self.writing.as_mut() else {
                return Poll::Ready(Ok(()));
            };
            if !writing.has_remaining() {
                self.writing = None;
                return Poll::Ready(Ok(()));
            }
            if !inner.detached.outbound_has_room() {
                inner.poll_timer(cx);
                return Poll::Pending;
            }
            match inner.send_chunk(self.stream, writing.chunk(), false)? {
                StreamWrite::Datagram { accepted, .. } => writing.advance(accepted),
                StreamWrite::Blocked
                | StreamWrite::StreamBlocked
                | StreamWrite::ConnectionBlocked
                | StreamWrite::Idle => {
                    inner.poll_timer(cx);
                    return Poll::Pending;
                }
            }
        }
        cx.waker().wake_by_ref();
        Poll::Pending
    }
}

impl<S: Session> quic::SendStream<Bytes> for SendStream<S> {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        self.poll_write(cx)
    }

    fn send_data<T: Into<WriteBuf<Bytes>>>(&mut self, data: T) -> Result<(), StreamErrorIncoming> {
        if self.writing.is_some() {
            return Err(StreamErrorIncoming::ConnectionErrorIncoming {
                connection_error: ConnectionErrorIncoming::InternalError(
                    "send_data called before poll_ready completed".into(),
                ),
            });
        }
        if self.finished {
            return Err(StreamErrorIncoming::Unknown(Box::new(BridgeError(
                "cannot write to a finished stream",
            ))));
        }
        self.writing = Some(data.into());
        Ok(())
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        if self.finished {
            return Poll::Ready(Ok(()));
        }
        if self.writing.is_some() {
            match self.poll_write(cx)? {
                Poll::Ready(()) => {}
                Poll::Pending => return Poll::Pending,
            }
        }

        let mut inner = self.shared.lock();
        inner.drive(cx).map_err(|connection_error| {
            StreamErrorIncoming::ConnectionErrorIncoming { connection_error }
        })?;
        if !inner.detached.outbound_has_room() {
            inner.poll_timer(cx);
            return Poll::Pending;
        }
        match inner.send_chunk(self.stream, &[], true)? {
            StreamWrite::Datagram { .. } => {
                self.finished = true;
                Poll::Ready(Ok(()))
            }
            StreamWrite::Blocked
            | StreamWrite::StreamBlocked
            | StreamWrite::ConnectionBlocked
            | StreamWrite::Idle => {
                inner.poll_timer(cx);
                Poll::Pending
            }
        }
    }

    fn reset(&mut self, reset_code: u64) {
        let mut inner = self.shared.lock();
        let _ = inner
            .detached
            .conn
            .reset_stream(self.stream, ApplicationErrorCode::new(reset_code));
        let _ = inner.produce();
        self.finished = true;
    }

    fn send_id(&self) -> H3StreamId {
        h3_stream(self.stream)
    }
}

/// The receiving half of an ngtcp2 stream.
pub struct RecvStream<S: Session> {
    shared: Arc<Shared<S>>,
    stream: StreamId,
    finished: bool,
}

impl<S: Session> RecvStream<S> {
    fn new(shared: Arc<Shared<S>>, stream: StreamId) -> Self {
        Self {
            shared,
            stream,
            finished: false,
        }
    }
}

impl<S: Session> quic::RecvStream for RecvStream<S> {
    type Buf = Bytes;

    fn poll_data(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
        if self.finished {
            return Poll::Ready(Ok(None));
        }
        let mut inner = self.shared.lock();
        let driven = inner.drive(cx);
        let (bytes, reset, finished) = {
            let state = inner.receive.entry(self.stream.get()).or_default();
            (state.chunks.pop_front(), state.reset, state.finished)
        };
        if let Some(bytes) = bytes {
            let consumed = bytes.len() as u64;
            if driven.is_ok() {
                inner
                    .detached
                    .conn
                    .extend_max_stream_offset(self.stream, consumed)
                    .map_err(stream_error)?;
                inner.detached.conn.extend_max_offset(consumed);
                inner.produce().map_err(|connection_error| {
                    StreamErrorIncoming::ConnectionErrorIncoming { connection_error }
                })?;
            }
            return Poll::Ready(Ok(Some(bytes)));
        }
        if let Some(error_code) = reset {
            self.finished = true;
            return Poll::Ready(Err(StreamErrorIncoming::StreamTerminated { error_code }));
        }
        if finished {
            self.finished = true;
            return Poll::Ready(Ok(None));
        }
        driven.map_err(
            |connection_error| StreamErrorIncoming::ConnectionErrorIncoming { connection_error },
        )?;
        Poll::Pending
    }

    fn stop_sending(&mut self, error_code: u64) {
        let mut inner = self.shared.lock();
        let _ = inner
            .detached
            .conn
            .stop_sending(self.stream, ApplicationErrorCode::new(error_code));
        let _ = inner.produce();
    }

    fn recv_id(&self) -> H3StreamId {
        h3_stream(self.stream)
    }
}

/// A bidirectional ngtcp2 stream.
pub struct BidiStream<S: Session> {
    send: SendStream<S>,
    recv: RecvStream<S>,
}

impl<S: Session> BidiStream<S> {
    fn new(shared: Arc<Shared<S>>, stream: StreamId) -> Self {
        Self {
            send: SendStream::new(Arc::clone(&shared), stream),
            recv: RecvStream::new(shared, stream),
        }
    }
}

impl<S: Session> quic::BidiStream<Bytes> for BidiStream<S> {
    type SendStream = SendStream<S>;
    type RecvStream = RecvStream<S>;

    fn split(self) -> (Self::SendStream, Self::RecvStream) {
        (self.send, self.recv)
    }
}

impl<S: Session> quic::SendStream<Bytes> for BidiStream<S> {
    fn poll_ready(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        quic::SendStream::<Bytes>::poll_ready(&mut self.send, cx)
    }

    fn send_data<T: Into<WriteBuf<Bytes>>>(&mut self, data: T) -> Result<(), StreamErrorIncoming> {
        quic::SendStream::<Bytes>::send_data(&mut self.send, data)
    }

    fn poll_finish(&mut self, cx: &mut Context<'_>) -> Poll<Result<(), StreamErrorIncoming>> {
        quic::SendStream::<Bytes>::poll_finish(&mut self.send, cx)
    }

    fn reset(&mut self, reset_code: u64) {
        quic::SendStream::<Bytes>::reset(&mut self.send, reset_code);
    }

    fn send_id(&self) -> H3StreamId {
        quic::SendStream::<Bytes>::send_id(&self.send)
    }
}

impl<S: Session> quic::RecvStream for BidiStream<S> {
    type Buf = Bytes;

    fn poll_data(
        &mut self,
        cx: &mut Context<'_>,
    ) -> Poll<Result<Option<Self::Buf>, StreamErrorIncoming>> {
        quic::RecvStream::poll_data(&mut self.recv, cx)
    }

    fn stop_sending(&mut self, error_code: u64) {
        quic::RecvStream::stop_sending(&mut self.recv, error_code);
    }

    fn recv_id(&self) -> H3StreamId {
        quic::RecvStream::recv_id(&self.recv)
    }
}

fn h3_stream(stream: StreamId) -> H3StreamId {
    (stream.get() as u64)
        .try_into()
        .expect("a valid QUIC stream ID is a valid h3 stream ID")
}

fn stream_error(error: ngnet_quic::Error) -> StreamErrorIncoming {
    StreamErrorIncoming::Unknown(Box::new(error))
}

#[derive(Debug)]
struct BridgeError(&'static str);

impl std::fmt::Display for BridgeError {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        formatter.write_str(self.0)
    }
}

impl std::error::Error for BridgeError {}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn stream_ids_keep_their_wire_value() {
        for value in [0, 1, 2, 3, 1 << 20, (1_i64 << 62) - 1] {
            let stream = StreamId::new(value).expect("valid QUIC stream ID");
            assert_eq!(h3_stream(stream), (value as u64).try_into().unwrap());
        }
    }
}
