// SPDX-License-Identifier: GPL-3.0-or-later

use alloc::vec::Vec;
use crate::fs::BlockDevice;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct MacAddress(pub [u8; 6]);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub struct Ipv4Address(pub [u8; 4]);

#[derive(Debug, Clone)]
pub struct NetConfig {
    pub ip: Ipv4Address,
    pub subnet: Ipv4Address,
    pub gateway: Ipv4Address,
    pub mac: MacAddress,
}

pub trait NetworkDevice {
    fn get_mac(&self) -> MacAddress;
    fn send_packet(&mut self, packet: &[u8]) -> Result<(), &'static str>;
    fn receive_packet(&mut self, buffer: &mut [u8]) -> Result<usize, &'static str>;
}

pub enum BootSource<'a> {
    Disk(&'a mut dyn BlockDevice),
    Network(&'a mut dyn NetworkDevice),
    Unknown,
}
