extern crate byteorder;
extern crate serde;
extern crate serde_eetf;

#[macro_use]
extern crate serde_derive;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};

pub struct MessageReader<'a> {
    reader: &'a mut Read,
}

impl<'a> MessageReader<'a> {
    pub fn new(reader: &'a mut Read) -> MessageReader<'a> {
        MessageReader { reader: reader }
    }
}

impl<'a> Iterator for MessageReader<'a> where {
    type Item = Vec<u8>;

    fn next(&mut self) -> Option<Vec<u8>> {
        match self.reader.read_u32::<BigEndian>() {
            Ok(packet_size) => {
                let mut buf = vec![0; packet_size as usize];
                self.reader
                    .read_exact(&mut buf)
                    .expect("Couldn't read full packet of data");
                Some(buf)
            }
            Err(err) => {
                if err.kind() == std::io::ErrorKind::UnexpectedEof {
                    return None;
                }
                panic!("IO when reading size {}", err);
            }
        }
    }
}

pub fn parse_input<'de, T>(buffer: Vec<u8>) -> T
where
    T: DeserializeOwned,
{
    serde_eetf::from_bytes(&buffer).expect("Deserialization Failed")
}

/// Writes an EETF Term into an io::Write along with it's size.
pub fn write_output<R, T, E>(result: R, output: &mut Write)
where
    R: Into<ErlResult<T, E>>,
    T: Serialize,
    E: Serialize,
{
    let erl_result: ErlResult<T, E> = result.into();
    let data = serde_eetf::to_bytes(&erl_result).expect("serialization failed");

    output
        .write_u32::<BigEndian>(data.len() as u32)
        .expect("write data size failed");

    output.write_all(&data).expect("writing result failed");
    output.flush().expect("flushing stdout failed");
}

#[derive(Serialize)]
pub enum ErlResult<T, E> {
    Ok(T),
    Error(E),
}

impl<T, E> From<Result<T, E>> for ErlResult<T, E> {
    fn from(result: Result<T, E>) -> Self {
        match result {
            Ok(success) => ErlResult::Ok(success),
            Err(error) => ErlResult::Error(error),
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn it_works() {
        assert_eq!(2 + 2, 4);
    }
}
