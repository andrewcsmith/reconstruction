extern crate bounded_spsc_queue;
extern crate portaudio;

use portaudio::{DeviceIndex, DeviceInfo};

pub enum DictionaryHandlerEvent {
    Refresh,
    Play,
    SetThreshold(usize),
    SetDepth(usize),
    InputBuffer(Option<bounded_spsc_queue::Consumer<[f32; 64]>>),
    Quit
}

#[derive(Debug)]
pub enum AudioHandlerEvent {
    Setting(DeviceSetting),
    Start,
    Stop,
    Quit
}

#[derive(Debug)]
pub enum DeviceSetting {
    SetInDevice(u32),
    SetOutDevice(u32),
}

pub enum GuiHandlerEvent {
    InDevice(usize),
    OutDevice(usize),
    Devices(Vec<(DeviceIndex, String)>),
}

