use crate::ProgramError;
use std::{char, str};
use tokio_util::bytes::BytesMut;
use tokio_util::codec::Decoder;

/// A lossy string decoder that replaces unrecognized characters with [`REPLACEMENT_CHAR`](std::char::REPLACEMENT_CHAR).
pub struct StringDecoder {
    /// An incomplete `char` value being decoded from the stream.
    /// `char`s are always four bytes in length.
    incomplete: (usize, [u8; 4]),
}

impl StringDecoder {
    pub const fn new() -> StringDecoder {
        StringDecoder {
            incomplete: (0, [0; 4]),
        }
    }
}

impl Decoder for StringDecoder {
    type Error = ProgramError;
    type Item = String;

    fn decode(&mut self, src: &mut BytesMut) -> Result<Option<Self::Item>, Self::Error> {
        if src.is_empty() {
            return Ok(None);
        }

        match str::from_utf8(src) {
            Err(err) if err.valid_up_to() > 0 => {
                // Split the bytes that are valid utf8 and turn it into &str.
                let split_bytes = src.split_to(err.valid_up_to());
                let valid_str = unsafe { str::from_utf8_unchecked(&split_bytes) };

                let (ref mut index, _) = self.incomplete;
                if *index > 0 {
                    // We have a partial character stored, but decoded a valid string
                    // after it, this means that this partial character cannot be
                    // completed so we replace it with a `REPLACEMENT_CHARACTER`.
                    let mut result = String::with_capacity(
                        valid_str.len() + char::REPLACEMENT_CHARACTER.len_utf8(),
                    );
                    result.push(char::REPLACEMENT_CHARACTER);
                    result.push_str(valid_str);
                    // Reset the incomplete index
                    *index = 0;

                    Ok(Some(result))
                } else {
                    Ok(Some(valid_str.to_owned()))
                }
            }
            Err(_) => {
                let (ref mut index, ref mut buf) = self.incomplete;

                // Index is always less than 4, because of below.
                buf[*index] = src.split_to(1)[0];
                *index += 1;

                // Check if char is valid
                if let Ok(s) = str::from_utf8(&buf[..*index]) {
                    // If valid turn it into string and return it.
                    let result = s.to_string();
                    // Reset index of `incomplete` because we've taken it.
                    *index = 0;

                    Ok(Some(result))
                } else if *index == 4 {
                    // Char is not valid, but the buffer is full, so return a
                    // replacement char.
                    *index = 0;
                    Ok(Some(char::REPLACEMENT_CHARACTER.to_string()))
                } else {
                    Ok(None)
                }
            }
            Ok(_) => {
                // Split the bytes used for `s`.
                let split_bytes = src.split();
                let s = unsafe { str::from_utf8_unchecked(&split_bytes) };

                let (ref mut index, _) = self.incomplete;
                if *index > 0 {
                    // We have a partial character stored, but decoded a valid string
                    // after it, this means that this partial character cannot be
                    // completed so we replace it with a `REPLACEMENT_CHARACTER`.
                    let mut result =
                        String::with_capacity(s.len() + char::REPLACEMENT_CHARACTER.len_utf8());
                    result.push(char::REPLACEMENT_CHARACTER);
                    result.push_str(s);
                    // Reset the incomplete index
                    *index = 0;

                    Ok(Some(result))
                } else {
                    Ok(Some(s.to_owned()))
                }
            }
        }
    }
}
