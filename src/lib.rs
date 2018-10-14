//! erlang_port helps make writing Erlang/Elixir ports in Rust easier.
//!
//! Makes use of the `serde_eetf` crate to serialize/deserialize rust datatypes
//! into erlang external term format, suitable for passing to/receiving from
//! `binary_to_term`/`term_to_binary`
//!
//! Assuming you are starting your port in packet mode, it's recommended that
//! you use the `stdio` or `nouse_stdio` functions inside the `main` fuction of
//! your application to create an `IOPort`. You can then use the `sender` and
//! `receiver` properties on that `IOPort` to communicate with Erlang/Elixir.
//!
//! For example, if you create the following rust program that reads strings
//! from a port and returns them uppercased:
//!
//! ```rust,no_run
//! fn lower(mut s: String) -> Result<String, String> {
//!     s.make_ascii_uppercase();
//!     Ok(s)
//! }
//!
//! fn main() {
//!    use erlang_port::{PortReceive, PortSend};
//!
//!   let mut port = unsafe {
//!       use erlang_port::PacketSize;
//!       erlang_port::nouse_stdio(PacketSize::Four)
//!   };
//!
//!   for string_in in port.receiver.iter() {
//!       let result = lower(string_in);
//!
//!       port.sender.reply(result);
//!   }
//! }
//! ```
//!
//! Then you can call into this port from Elixir:
//!
//! ```elixir
//! iex> port =
//! ...>   Port.open({:spawn_executable, port_path}, [
//! ...>       {:packet, 4},
//! ...>       :nouse_stdio,
//! ...>       :binary,
//! ...>       :exit_status
//! ...>   ])
//! #Port<0.1444>
//! iex> Port.command(port, :erlang.term_to_binary("hello"))
//! true
//! iex> receive do
//! ...>   {^port, {:data, binary}} ->
//! ...>     IO.puts(:erlang.binary_to_term(binary))
//! ...> end
//! "HELLO"
//! :ok
//! ```
//!
//! If you wish to implement a line-based port or a custom port protocol (using
//! the :stream option) you can do so by implementing the
//! `PortSend`/`PortReceive` traits.

extern crate byteorder;
extern crate serde;
extern crate serde_eetf;

#[macro_use]
extern crate serde_derive;

use byteorder::BigEndian;
use serde::de::DeserializeOwned;
use serde::Serialize;
use std::io::{Read, Write};
use std::marker::PhantomData;

/// Creates an IOPort using stdin & stdout with the specified packet size.
///
/// This should be used with ports that have been opened in Erlang/Elixir with
/// the `:use_stdio` and `{:packet, N}` options, where N matches packet_size.
///
/// Note: at present we don't do any locking of the underlying stdin or stdout
/// streams, so you should only call this once in your port. This may be changed
/// in the future...
pub fn stdio(
    packet_size: PacketSize,
) -> IOPort<PacketReceiver<std::io::Stdin>, PacketSender<std::io::Stdout>> {
    IOPort {
        receiver: PacketReceiver::from_reader(std::io::stdin(), packet_size),
        sender: PacketSender::from_writer(std::io::stdout(), packet_size),
    }
}

/// Creates an IOPort using file descriptors 3 and 4 with the specified packet size.
///
/// This allows the port application to continue to use stdout for logging/other
/// output.
///
/// This should be used with ports that have been opened in Erlang/Elixir with
/// the `:nouse_stdio` and `{:packet, N}` options, where N matches packet_size.
///
/// This function is unsafe, and if you call this function for a port that was
/// not created with nouse_stdio then it will panic.
pub unsafe fn nouse_stdio(
    packet_size: PacketSize,
) -> IOPort<PacketReceiver<std::fs::File>, PacketSender<std::fs::File>> {
    use std::fs::File;
    use std::os::unix::io::FromRawFd;

    IOPort {
        receiver: PacketReceiver::from_reader(File::from_raw_fd(3), packet_size),
        sender: PacketSender::from_writer(File::from_raw_fd(4), packet_size),
    }
}

/// `PortReceive` allows an app to receive messages through an Erlang port.
pub trait PortReceive {
    /// Receives a single message over the port.
    ///
    /// If there are no more messages returns None. If there's a problem reading
    /// the message it will panic.
    ///
    /// A `MessageDeserialize` implementation shold exist for the message we are
    /// reading. We provide a `serde_eetf` based default implementation for any
    /// type implementing serdes `DeserializeOwned`.
    fn receive<T>(&mut self) -> Option<T>
    where
        T: MessageDeserialize;

    /// Creates an Iterator over a series of messages read from the port.
    fn iter<'a, T>(&'a mut self) -> MessageIterator<'a, Self, T>
    where
        Self: Sized,
    {
        MessageIterator::from_receiver(self)
    }
}

/// `PortSend` allows an application to send messages through an Erlang port.
pub trait PortSend {
    /// Sends a single message over the port.
    ///
    /// A `MessageSerialize` implementation shold exist for the message we are
    /// reading. We provide a `serde_eetf` based default implementation for any
    /// type implementing serdes `Serialize`.
    ///
    /// In Erlang "Let it Crash" style, if we fail to encode the command or write it
    /// to a stream we will panic. Since this library is intended to be used as part
    /// of an Erlang system this should be picked up by the BEAM VM which can
    /// restart the Port.
    ///
    /// It's possible that panicing is _not_ what you want, for example if you have
    /// a Port that is handling multiple commands concurrently. Feel free to make a
    /// PR to better support your use case if that is the case.
    fn send<T>(&mut self, data: T)
    where
        T: MessageSerialize;

    /// Sends an `{:ok, T}` or `{:error, E}` over the port.
    ///
    /// This can be used to reply to a command that was received in the port, or to
    /// send arbitrary commands to the Port inside the BEAM.
    ///
    /// Applications can pass any data type they want into `reply`, provided there's
    /// a definition of `Into<ErlResult<E, T>>` for that type. A default
    /// implementation is provided for `Result<E, T>`.
    ///
    /// Note that both E and T must implement `Serialize` rather than
    /// `MessageSerialize` as is the case for `send`.
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
    fn reply<R, T, E>(&mut self, response: R)
    where
        R: Into<ErlResult<T, E>>,
        T: Serialize,
        E: Serialize,
    {
        self.send::<ErlResult<T, E>>(response.into());
    }
}

/// The size of a packet as sent/received by a PacketReceiver or PacketSender
///
/// You should pick the PacketSize that corresponds to the `{:packet, N}` option
/// you are opening the port with in Erlang/Elixir.
#[derive(Clone, Copy)]
pub enum PacketSize {
    One,
    Two,
    Four,
}

/// A receiver for ports opened in Packet mode.
///
/// If a port is opened with the `{:packet, N}` option then this receiver can
/// be used with the `packet_size` set to `N`
pub struct PacketReceiver<R>
where
    R: Read,
{
    reader: R,
    packet_size: PacketSize,
}

impl<R> PacketReceiver<R>
where
    R: Read,
{
    pub fn from_reader(reader: R, packet_size: PacketSize) -> Self {
        PacketReceiver {
            reader: reader,
            packet_size: packet_size,
        }
    }

    fn read_size(&mut self) -> Result<usize, std::io::Error> {
        use byteorder::ReadBytesExt;

        match self.packet_size {
            PacketSize::One => self.reader.read_u8().map(|n| n as usize),
            PacketSize::Two => self.reader.read_u16::<BigEndian>().map(|n| n as usize),
            PacketSize::Four => self.reader.read_u32::<BigEndian>().map(|n| n as usize),
        }
    }
}

impl<R> PortReceive for PacketReceiver<R>
where
    R: Read,
{
    fn receive<T>(&mut self) -> Option<T>
    where
        T: MessageDeserialize,
    {
        match self.read_size() {
            Ok(packet_size) => {
                let mut buf = vec![0; packet_size];
                self.reader
                    .read_exact(&mut buf)
                    .expect("Couldn't read full packet of data");
                Some(MessageDeserialize::deserialize_message(buf))
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

/// A sender for ports opened in Packet mode.
///
/// If a port is opened with the `{:packet, N}` option then this sender can
/// be used with the `packet_size` set to `N`
pub struct PacketSender<W>
where
    W: Write,
{
    writer: W,
    packet_size: PacketSize,
}

impl<W> PacketSender<W>
where
    W: Write,
{
    pub fn from_writer(writer: W, packet_size: PacketSize) -> Self {
        PacketSender {
            writer: writer,
            packet_size: packet_size,
        }
    }

    fn write_size(&mut self, size: usize) -> Result<(), std::io::Error> {
        use byteorder::WriteBytesExt;

        // TODO: Should probably verify size fits within packet_size here...

        match self.packet_size {
            PacketSize::One => self.writer.write_u8(size as u8),
            PacketSize::Two => self.writer.write_u16::<BigEndian>(size as u16),
            PacketSize::Four => self.writer.write_u32::<BigEndian>(size as u32),
        }
    }
}

impl<W> PortSend for PacketSender<W>
where
    W: Write,
{
    fn send<T>(&mut self, message: T)
    where
        T: MessageSerialize,
    {
        let data = message.serialize_message();

        self.write_size(data.len()).expect("write data size failed");
        self.writer.write_all(&data).expect("writing result failed");
        self.writer.flush().expect("flushing stdout failed");
    }
}

/// A wrapper around a receiver and a sender.
///
/// This struct does not implement PortSend or PortReceive itself, as you could
/// end up mutably borrowing the IOPort when calling `port.iter()` and then
/// you'd be unable to call `port.reply` inside your loop. Instead you should
/// call the functions on receiver and sender directly. This may be changed in a
/// future release if I figure out a way to do it nicely.
pub struct IOPort<R, S>
where
    R: PortReceive,
    S: PortSend,
{
    pub receiver: R,
    pub sender: S,
}

/// Iterator over messages from a PortReceive.
pub struct MessageIterator<'a, R: 'a, T>
where
    R: PortReceive,
{
    receiver: &'a mut R,
    phantom: PhantomData<T>,
}

impl<'a, R, T> MessageIterator<'a, R, T>
where
    R: PortReceive,
{
    pub fn from_receiver(receiver: &'a mut R) -> MessageIterator<'a, R, T> {
        MessageIterator {
            receiver: receiver,
            phantom: PhantomData,
        }
    }
}

impl<'a, R, T> Iterator for MessageIterator<'a, R, T>
where
    R: PortReceive,
    T: MessageDeserialize,
{
    type Item = T;

    fn next(&mut self) -> Option<T> {
        self.receiver.receive()
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
/// PR or raise an issue to better support your use case if that is the case.
pub trait MessageDeserialize {
    fn deserialize_message(buffer: Vec<u8>) -> Self;
}

impl<T> MessageDeserialize for T
where
    T: DeserializeOwned,
{
    fn deserialize_message(buffer: Vec<u8>) -> Self {
        serde_eetf::from_bytes(&buffer).expect("Deserialization Failed")
    }
}

/// Trait that serializes some data into a Vec<u8>
///
/// This is used in the send function to serialize commands. A default
/// implementation is provided for anything implementing Serialize from
/// serde.
///
/// In Erlang "Let it Crash" style, if we fail to serialize for whatever reason
/// trait shoul panic. Since this library is intended to be used as part of an
/// Erlang system this should be picked up by the BEAM VM which can restart the
/// Port.
///
/// It's possible that panicing is _not_ what you want, for example if you have
/// a Port that is handling multiple commands concurrently. Feel free to make a
/// PR or raise an issue to better support your use case if that is the case.
pub trait MessageSerialize {
    fn serialize_message(self) -> Vec<u8>;
}

impl<T> MessageSerialize for T
where
    T: Serialize,
{
    fn serialize_message(self: Self) -> Vec<u8> {
        serde_eetf::to_bytes(&self).expect("Serialization failed")
    }
}

/// A result enum for replying to commands from Erlang.
///
/// This will serialize into a standard erlang result tuple of either:
///
/// 1. `{:ok, result}` on Ok
/// 2. `{:error err}` on Error
///
/// All replies sent via `reply` are converted into this enum.
#[derive(Serialize, Deserialize, Debug, PartialEq)]
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
    use super::*;

    use byteorder::WriteBytesExt;
    use serde_eetf;
    use std::io::Cursor;

    #[derive(Serialize, Deserialize, Debug, PartialEq)]
    struct TestCommand {
        int8: i8,
    }

    #[test]
    fn test_packet_receiver_iter() {
        let buff = serde_eetf::to_bytes(&TestCommand { int8: 100 }).unwrap();

        let mut size_buff = Vec::new();
        size_buff
            .write_u32::<BigEndian>(buff.len() as u32)
            .expect("write data size failed");
        size_buff.extend_from_slice(&buff);
        size_buff
            .write_u32::<BigEndian>(buff.len() as u32)
            .expect("write data size failed");
        size_buff.extend_from_slice(&buff);

        let results: Vec<TestCommand> =
            PacketReceiver::from_reader(&mut Cursor::new(size_buff), PacketSize::Four)
                .iter()
                .collect();
        assert_eq!(results.len(), 2);
        assert_eq!(results[0], results[1]);
    }

    #[test]
    fn test_packet_sender_reply() {
        fn do_test(input: Result<i8, String>, expected_output: ErlResult<i8, String>) {
            let mut cursor = Cursor::new(vec![]);
            {
                let mut sender = PacketSender::from_writer(&mut cursor, PacketSize::Four);
                sender.reply(input);
            }
            cursor.set_position(0);

            let result: Vec<ErlResult<i8, String>> =
                PacketReceiver::from_reader(&mut cursor, PacketSize::Four)
                    .iter()
                    .collect();

            assert_eq!(result, [expected_output]);
        }

        do_test(Ok(1), ErlResult::Ok(1));
        do_test(
            Err("Nope".to_string()),
            ErlResult::Error("Nope".to_string()),
        );
    }

    #[test]
    fn test_packet_sender_send() {
        fn do_test(packet_size: PacketSize) {
            let input = TestCommand { int8: 127 };

            let mut cursor = Cursor::new(vec![]);
            {
                let mut sender = PacketSender::from_writer(&mut cursor, packet_size);
                sender.send(&input);
            }
            cursor.set_position(0);

            let result: Vec<TestCommand> = PacketReceiver::from_reader(&mut cursor, packet_size)
                .iter()
                .collect();

            assert_eq!(result, [input]);
        }

        do_test(PacketSize::One);
        do_test(PacketSize::Two);
        do_test(PacketSize::Four);
    }
}
