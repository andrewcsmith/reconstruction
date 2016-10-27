#[allow(dead_code)]
pub enum DictionaryHandlerEvent {
    Refresh,
    Play,
    SetThreshold(usize),
    SetDepth(usize),
    Quit
}

pub enum AudioHandlerEvent {
    Quit
}

