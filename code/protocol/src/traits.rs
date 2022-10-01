/// A general Serialize/Deserialize trait to make composition of types easier
pub trait Sendable<'r>: Sized {
    type SerError;
    type DeSerError;

    /// Serializes the object into the given buffer and returns the remaining buffer, that can be
    /// used for storing other information
    fn serialize<'b>(&self, buffer: &'b mut [u8]) -> Result<&'b mut [u8], Self::SerError>;

    /// Attempts to deserialize the given Buffer into an instance of itself and returns that and
    /// any remaining buffer, that might contain other data
    fn deserialize(buffer: &'r [u8]) -> Result<(Self, &'r [u8]), Self::DeSerError>;
}

impl<'r> Sendable<'r> for &'r str {
    type SerError = ();
    type DeSerError = ();

    fn serialize<'b>(&self, buffer: &'b mut [u8]) -> Result<&'b mut [u8], Self::SerError> {
        if buffer.len() < self.len() + 1 {
            return Err(());
        }

        buffer[0] = self.len() as u8;
        buffer[1..(1 + self.len())].copy_from_slice(self.as_bytes());

        Ok(&mut buffer[(1 + self.len())..])
    }

    fn deserialize(buffer: &'r [u8]) -> Result<(Self, &'r [u8]), Self::DeSerError> {
        let len = buffer[0] as usize;
        let value = core::str::from_utf8(&buffer[1..(len + 1)]).expect("");

        Ok((value, &buffer[(1 + len)..]))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn str_sendable() {
        let mut buffer = [0; 100];

        let content = "testing stuff";

        content.serialize(&mut buffer).expect("Should work");

        let (deserialized, _): (&str, _) = Sendable::deserialize(&buffer).expect("Should work");

        assert_eq!(content, deserialized);
    }

    #[test]
    fn str_serialize_buffer_too_small() {
        let mut buffer = [0; 3];
        let content = "testing";
        assert!(content.serialize(&mut buffer).is_err());
    }
}
