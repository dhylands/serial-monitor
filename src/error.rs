use std::fmt;

pub enum ProgramError {
    NoPortFound,
    UnableToOpen(String, std::io::Error),
    IoError(std::io::Error),
    SerialPortError(serialport::Error),
    CrossTermError,
}

impl std::error::Error for ProgramError {}

impl From<crossterm::ErrorKind> for ProgramError {
    fn from(_err: crossterm::ErrorKind) -> ProgramError {
        ProgramError::CrossTermError
    }
}

impl From<std::io::Error> for ProgramError {
    fn from(err: std::io::Error) -> ProgramError {
        ProgramError::IoError(err)
    }
}

impl From<serialport::Error> for ProgramError {
    fn from(err: serialport::Error) -> ProgramError {
        ProgramError::SerialPortError(err)
    }
}

impl fmt::Debug for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter) -> fmt::Result {
        match &*self {
            ProgramError::NoPortFound => {
                write!(f, "No USB serial adapter found which matches criteria.")
            }
            ProgramError::UnableToOpen(port_name, err) => {
                write!(f, "Unable to open serial port '{}': {}", port_name, err)
            }
            ProgramError::IoError(err) => write!(f, "{}", err),
            ProgramError::CrossTermError => write!(f, "Crossterm error"),
            ProgramError::SerialPortError(err) => write!(f, "SerialPortError: {}", err),
        }
    }
}
impl fmt::Display for ProgramError {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        write!(f, "{:?}", self)
    }
}
pub type Result<T> = std::result::Result<T, ProgramError>;
