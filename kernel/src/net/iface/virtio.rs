// SPDX-License-Identifier: MPL-2.0

use aster_network::AnyNetworkDevice;
use aster_virtio::device::network::DEVICE_NAME;
use ostd::sync::PreemptDisabled;
use smoltcp::{
    iface::{Config, SocketHandle, SocketSet},
    socket::dhcpv4,
    wire::{self, IpCidr},
};

use super::{common::IfaceCommon, internal::IfaceInternal, time::get_network_timestamp, Iface};
use crate::prelude::*;

pub struct IfaceVirtio {
    driver: Arc<SpinLock<dyn AnyNetworkDevice, PreemptDisabled>>,
    common: IfaceCommon,
    dhcp_handle: SocketHandle,
    weak_self: Weak<Self>,
}

impl IfaceVirtio {
    pub fn new() -> Arc<Self> {
        let virtio_net = aster_network::get_device(DEVICE_NAME).unwrap();
        let interface = {
            let mac_addr = virtio_net.lock().mac_addr();
            let ip_addr = IpCidr::new(wire::IpAddress::Ipv4(wire::Ipv4Address::UNSPECIFIED), 0);
            let config = Config::new(wire::HardwareAddress::Ethernet(wire::EthernetAddress(
                mac_addr.0,
            )));
            let now = get_network_timestamp();

            let mut interface =
                smoltcp::iface::Interface::new(config, &mut *virtio_net.lock(), now);
            interface.update_ip_addrs(|ip_addrs| {
                debug_assert!(ip_addrs.is_empty());
                ip_addrs.push(ip_addr).unwrap();
            });
            interface
        };
        let common = IfaceCommon::new(interface);
        let mut socket_set = common.sockets();
        let dhcp_handle = init_dhcp_client(&mut socket_set);
        drop(socket_set);
        Arc::new_cyclic(|weak| Self {
            driver: virtio_net,
            common,
            dhcp_handle,
            weak_self: weak.clone(),
        })
    }

    /// FIXME: Once we have user program dhcp client, we may remove dhcp logic from kernel.
    pub fn process_dhcp(&self) {
        let mut socket_set = self.common.sockets();
        let dhcp_socket: &mut dhcpv4::Socket = socket_set.get_mut(self.dhcp_handle);
        let config = if let Some(event) = dhcp_socket.poll() {
            debug!("event = {:?}", event);
            if let dhcpv4::Event::Configured(config) = event {
                config
            } else {
                return;
            }
        } else {
            return;
        };
        let ip_addr = IpCidr::Ipv4(config.address);
        let mut interface = self.common.interface();
        interface.update_ip_addrs(|ipaddrs| {
            if let Some(addr) = ipaddrs.iter_mut().next() {
                // already has ipaddrs
                *addr = ip_addr
            } else {
                // does not has ip addr
                ipaddrs.push(ip_addr).unwrap();
            }
        });
        println!(
            "DHCP update IP address: {:?}",
            interface.ipv4_addr().unwrap()
        );
        if let Some(router) = config.router {
            println!("Default router address: {:?}", router);
            interface
                .routes_mut()
                .add_default_ipv4_route(router)
                .unwrap();
        }
    }
}

impl IfaceInternal for IfaceVirtio {
    fn common(&self) -> &IfaceCommon {
        &self.common
    }

    fn arc_self(&self) -> Arc<dyn Iface> {
        self.weak_self.upgrade().unwrap()
    }
}

impl Iface for IfaceVirtio {
    fn name(&self) -> &str {
        "virtio"
    }

    fn mac_addr(&self) -> Option<smoltcp::wire::EthernetAddress> {
        let interface = self.common.interface();
        let hardware_addr = interface.hardware_addr();
        match hardware_addr {
            wire::HardwareAddress::Ethernet(ethe_address) => Some(ethe_address),
            wire::HardwareAddress::Ip => None,
        }
    }

    fn poll(&self) {
        let mut driver = self.driver.disable_irq().lock();
        self.common.poll(&mut *driver);
        self.process_dhcp();
    }
}

/// Register a dhcp socket.
fn init_dhcp_client(socket_set: &mut SocketSet) -> SocketHandle {
    let dhcp_socket = dhcpv4::Socket::new();
    socket_set.add(dhcp_socket)
}
