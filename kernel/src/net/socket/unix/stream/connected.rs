// SPDX-License-Identifier: MPL-2.0

use crate::{
    events::{IoEvents, Observer},
    fs::utils::{Channel, Consumer, Producer},
    net::socket::{unix::addr::UnixSocketAddrBound, SockShutdownCmd},
    prelude::*,
    process::signal::Poller,
};

pub(super) struct Connected {
    addr: Option<UnixSocketAddrBound>,
    peer_addr: Option<UnixSocketAddrBound>,
    reader: Consumer<u8>,
    writer: Producer<u8>,
}

impl Connected {
    pub(super) fn new_pair(
        addr: Option<UnixSocketAddrBound>,
        peer_addr: Option<UnixSocketAddrBound>,
    ) -> (Connected, Connected) {
        let (writer_this, reader_peer) = Channel::with_capacity(DEFAULT_BUF_SIZE).split();
        let (writer_peer, reader_this) = Channel::with_capacity(DEFAULT_BUF_SIZE).split();

        let this = Connected {
            addr: addr.clone(),
            peer_addr: peer_addr.clone(),
            reader: reader_this,
            writer: writer_this,
        };
        let peer = Connected {
            addr: peer_addr,
            peer_addr: addr,
            reader: reader_peer,
            writer: writer_peer,
        };

        (this, peer)
    }

    pub(super) fn addr(&self) -> Option<&UnixSocketAddrBound> {
        self.addr.as_ref()
    }

    pub(super) fn peer_addr(&self) -> Option<&UnixSocketAddrBound> {
        self.peer_addr.as_ref()
    }

    pub(super) fn try_read(&self, buf: &mut [u8]) -> Result<usize> {
        let mut writer = VmWriter::from(buf).to_fallible();
        self.reader.try_read(&mut writer)
    }

    pub(super) fn try_write(&self, buf: &[u8]) -> Result<usize> {
        let mut reader = VmReader::from(buf).to_fallible();
        self.writer.try_write(&mut reader)
    }

    pub(super) fn shutdown(&self, cmd: SockShutdownCmd) -> Result<()> {
        // FIXME: If the socket has already been shut down, should we return an error code?

        if cmd.shut_read() {
            self.reader.shutdown();
        }

        if cmd.shut_write() {
            self.writer.shutdown();
        }

        Ok(())
    }

    pub(super) fn poll(&self, mask: IoEvents, mut poller: Option<&mut Poller>) -> IoEvents {
        let mut events = IoEvents::empty();

        // FIXME: should reader and writer use the same mask?
        let reader_events = self.reader.poll(mask, poller.as_deref_mut());
        let writer_events = self.writer.poll(mask, poller);

        // FIXME: Check this logic later.
        if reader_events.contains(IoEvents::HUP) || self.reader.is_shutdown() {
            events |= IoEvents::RDHUP | IoEvents::IN;
            if writer_events.contains(IoEvents::ERR) || self.writer.is_shutdown() {
                events |= IoEvents::HUP | IoEvents::OUT;
            }
        }

        events |= (reader_events & IoEvents::IN) | (writer_events & IoEvents::OUT);

        events
    }

    pub(super) fn register_observer(
        &self,
        observer: Weak<dyn Observer<IoEvents>>,
        mask: IoEvents,
    ) -> Result<()> {
        if mask.contains(IoEvents::IN) {
            self.reader.register_observer(observer.clone(), mask)?
        }

        if mask.contains(IoEvents::OUT) {
            self.writer.register_observer(observer, mask)?
        }

        Ok(())
    }

    pub(super) fn unregister_observer(
        &self,
        observer: &Weak<dyn Observer<IoEvents>>,
    ) -> Option<Weak<dyn Observer<IoEvents>>> {
        let observer0 = self.reader.unregister_observer(observer);
        let observer1 = self.writer.unregister_observer(observer);

        if observer0.is_some() {
            observer0
        } else if observer1.is_some() {
            observer1
        } else {
            None
        }
    }
}

const DEFAULT_BUF_SIZE: usize = 65536;
