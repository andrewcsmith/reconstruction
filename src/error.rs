use std;
use conrod;
use portaudio;

#[derive(Debug)]
pub enum Error<T> {
    PortAudio(portaudio::Error),
    Font(conrod::text::font::Error),
    SendError(std::sync::mpsc::SendError<T>),
    String(String)
}

impl<T> From<String> for Error<T> {
    fn from(string: String) -> Error<T> {
        Error::String(string)
    }
}

impl<T> From<portaudio::Error> for Error<T> {
    fn from(pa_error: portaudio::Error) -> Error<T> {
        Error::PortAudio(pa_error)
    }
}

impl<T> From<conrod::text::font::Error> for Error<T> {
    fn from(err: conrod::text::font::Error) -> Error<T> {
        Error::Font(err)
    }
}

impl<T> From<std::sync::mpsc::SendError<T>> for Error<T> {
    fn from(send_error: std::sync::mpsc::SendError<T>) -> Error<T> {
        Error::SendError(send_error)
    }
}

impl<T> std::fmt::Display for Error<T> {
    fn fmt(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
        match *self {
            Error::PortAudio(ref err) => write!(f, "PortAudio error: {}", err),
            Error::Font(ref err) => write!(f, "Font error: {}", err),
            Error::SendError(ref err) => write!(f, "Send error: {}", err),
            Error::String(ref err) => write!(f, "String error: {}", err)
        }
    }
}

impl<T: std::marker::Send + std::fmt::Debug> std::error::Error for Error<T> {
    fn description(&self) -> &str {
        match *self {
            Error::PortAudio(ref err) => err.description(),
            Error::Font(ref err) => err.description(),
            Error::SendError(ref err) =>  err.description(),
            Error::String(ref err) => err
        }
    }

    fn cause(&self) -> Option<&std::error::Error> {
        match *self {
            Error::PortAudio(ref err) => Some(err),
            Error::Font(ref err) => Some(err),
            Error::SendError(ref err) => Some(err),
            Error::String(_) => None
        }
    }
}



