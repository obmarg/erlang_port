extern crate byteorder;
extern crate serde;
extern crate serde_eetf;

#[macro_use]
extern crate serde_derive;

use byteorder::{BigEndian, ReadBytesExt, WriteBytesExt};
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};
use std::marker::PhantomData;

/// An iterator over Erlang port messages from a stream.
///
/// This will read a series of messages from an Erlang port started in packet
/// mode with 4 byte packet sizes. Each message will be returned as a Vec<u8>,
/// which can be decoded using the parse_input function..
///
/// For example, to start a port and send a command in Elixir:
///
/// ```elixir
/// port = Port.open(
///  {:spawn_executable, port_path},
///  [{:packet, 4}, :use_stdio, :binary, :exit_status]
/// )
/// Port.command(port, "Hello!")
/// ```
///
/// A CommandIter that was reading the stdin of this Port would receive a
/// Vec<u8> with the UTF-8 bytes `Hello!` in it.
pub struct CommandIter<'a, T>
{
    reader: &'a mut Read,
    phantom: PhantomData<T>
}

impl<'a, T> CommandIter<'a, T> {
    pub fn from_reader(reader: &'a mut Read) -> CommandIter<'a, T> {
        CommandIter { reader: reader, phantom: PhantomData }
    }
}

impl<'a, T> Iterator for CommandIter<'a, T>
where T: CommandParser
    {

    type Item = T;

    fn next(&mut self) -> Option<T> {
        receive(self.reader)
    }
}

/// Trait that parses some data from a Vec<u8>
///
/// This is used in the receive function to deserialize commands. A default
/// implementation is provided for anything implementing DeserializeOwned from
/// serde.
///
/// In Erlang "Let it Crash" style, if the data in `buffer` is malformed this
/// trait shoul panic. Since this library is intended to be used as part of an
/// Erlang system this should be picked up by the BEAM VM which can restart the
/// Port.
///
/// It's possible that panicing is _not_ what you want, for example if you have
/// a Port that is handling multiple commands concurrently. Feel free to make a
/// PR to better support your use case if that is the case.
pub trait CommandParser {
    fn parse_command<>(buffer: Vec<u8>) -> Self;
}

impl<T> CommandParser for T
where T: DeserializeOwned
{
    fn parse_command(buffer: Vec<u8>) -> Self {
        serde_eetf::from_bytes(&buffer).expect("Deserialization Failed")
    }
}

/// Reads a single port message from a stream.
///
/// Attempts to read a single message from a Read. If there are no more messages
/// returns None. if there's a problem reading the message it will panic.
///
/// Each message is expected to start with a 4 byte message size, as happens
/// when you open an Erlang port in `{:packet, 4}` mode.
pub fn receive<T>(reader: &mut Read) -> Option<T>
where T: CommandParser
{
    match reader.read_u32::<BigEndian>() {
        Ok(packet_size) => {
            let mut buf = vec![0; packet_size as usize];
            reader
                .read_exact(&mut buf)
                .expect("Couldn't read full packet of data");
            Some(CommandParser::parse_command(buf))
        }
        Err(err) => {
            if err.kind() == std::io::ErrorKind::UnexpectedEof {
                return None;
            }
            panic!("IO when reading size {}", err);
        }
    }

}

/// Writes an EETF Term into an io::Write along with it's size.
///
/// This can be used to send arbitrary commands to the Port inside the BEAM.
///
/// In Erlang "Let it Crash" style, if we fail to encode the command or write it
/// to a stream we will panic. Since this library is intended to be used as part
/// of an Erlang system this should be picked up by the BEAM VM which can
/// restart the Port.
///
/// It's possible that panicing is _not_ what you want, for example if you have
/// a Port that is handling multiple commands concurrently. Feel free to make a
/// PR to better support your use case if that is the case.
pub fn send<T>(output: &mut Write, data: T)
where
    T: Serialize,
{
    let data = serde_eetf::to_bytes(&data).expect("serialization failed");

    output
        .write_u32::<BigEndian>(data.len() as u32)
        .expect("write data size failed");

    output.write_all(&data).expect("writing result failed");
    output.flush().expect("flushing stdout failed");
}

/// Writes a reply into an io::Write along with it's size.
///
/// This can be used to reply to a command that was received in the port, or to
/// send arbitrary commands to the Port inside the BEAM.
///
/// Applications can pass any data type they want into `reply`, provided there's
/// a definition of `Into<ErlResult<E, T>>` for that type. A default
/// implementation is provided for `Result<E, T>`.
///
/// If you wish to return a custom type you can implement `Into<ErlResult<T,
/// E>>` for that type, or use the `send` function instead to send a custom
/// type.
///
/// At the moment this function is just a simple wrapper around `send` but that
/// may change in the future.
///
/// In Erlang "Let it Crash" style, if we fail to encode the command or write it
/// to a stream we will panic. Since this library is intended to be used as part
/// of an Erlang system this should be picked up by the BEAM VM which can
/// restart the Port.
///
/// It's possible that panicing is _not_ what you want, for example if you have
/// a Port that is handling multiple commands concurrently. Feel free to make a
/// PR to better support your use case if that is the case.
pub fn reply<R, T, E>(output: &mut Write, response: R)
where
    R: Into<ErlResult<T, E>>,
    T: Serialize,
    E: Serialize
{
    send::<ErlResult<T, E>>(output, response.into());
}

/// A result enum for replying to commands from Erlang.
///
/// This will serialize into a standard erlang result tuple of either:
///
/// 1. `{:ok, result}` on Ok
/// 2. `{:error err}` on Error
///
/// All replies sent via `reply` are converted into this enum.
#[derive(Serialize)]
pub enum ErlResult<T, E>
where T: Serialize,
      E: Serialize
{
    Ok(T),
    Error(E),
}

impl<T, E> From<Result<T, E>> for ErlResult<T, E>
where T: Serialize,
      E: Serialize
{
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
