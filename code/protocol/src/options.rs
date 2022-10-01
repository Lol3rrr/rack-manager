use crate::Sendable;

/// The Values possible for Configuration-Options and Metrics
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum Value {
    Switch { state: bool },
    Pwm { percent: u8 },
}

#[derive(Debug, PartialEq, Eq)]
pub enum ValueDeserializeError {
    UnknownType(u8),
}

impl Value {
    pub fn serialize(&self) -> [u8; 2] {
        let mut buffer = [0; 2];

        match self {
            Self::Switch { state } => {
                buffer[0] = 0;
                buffer[1] = u8::from(*state);
            }
            Self::Pwm { percent } => {
                buffer[0] = 1;
                buffer[1] = *percent;
            }
        };

        buffer
    }

    pub fn deserialize(buffer: &[u8; 2]) -> Result<Self, ValueDeserializeError> {
        match buffer[0] {
            0 => {
                let state = buffer[1] == 1;
                Ok(Self::Switch { state })
            }
            1 => {
                let percent = buffer[1];
                Ok(Self::Pwm { percent })
            }
            val => Err(ValueDeserializeError::UnknownType(val)),
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum DataPointDeserializeError {
    ValueError(ValueDeserializeError),
    Other,
}

impl From<ValueDeserializeError> for DataPointDeserializeError {
    fn from(e: ValueDeserializeError) -> Self {
        Self::ValueError(e)
    }
}
impl From<()> for DataPointDeserializeError {
    fn from(_: ()) -> Self {
        Self::Other
    }
}

/// A combination of Name and Value, that can be used to represent a Metric or a Configuration
/// depending on the Context
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct DataPoint<'r> {
    /// The Name for the DataPoint
    pub name: &'r str,
    /// The Value of this DataPoint
    pub value: Value,
}

impl<'r> Sendable<'r> for DataPoint<'r> {
    type SerError = ();
    type DeSerError = DataPointDeserializeError;

    fn serialize<'b>(&self, mut buffer: &'b mut [u8]) -> Result<&'b mut [u8], Self::SerError> {
        buffer = self.name.serialize(buffer)?;

        if buffer.len() < 2 {
            return Err(());
        }
        buffer[0..2].copy_from_slice(&self.value.serialize());

        Ok(&mut buffer[2..])
    }

    fn deserialize(buffer: &'r [u8]) -> Result<(Self, &'r [u8]), Self::DeSerError> {
        let (name, buffer) = Sendable::deserialize(buffer)?;
        let value = Value::deserialize(buffer[0..2].try_into().unwrap())?;

        Ok((Self { name, value }, &buffer[2..]))
    }
}

#[derive(Debug, PartialEq, Eq, Clone)]
pub enum ValueType {
    Switch,
    Pwm,
}

/// A single Configuration option provided by an Extension-Board. This allows you to communicate
/// possible configurations to the Controller and therefore allow for more/runtime customization.
#[derive(Debug, PartialEq, Eq, Clone)]
pub struct ConfigOption<'r> {
    /// The Name of the Options, this should be a unique Name for this Board
    pub name: &'r str,
    /// The Type of Option
    pub ty: ValueType,
}

impl<'r> Sendable<'r> for ConfigOption<'r> {
    type SerError = ();
    type DeSerError = ();

    fn serialize<'b>(&self, buffer: &'b mut [u8]) -> Result<&'b mut [u8], Self::SerError> {
        let rest = self.name.serialize(buffer)?;
        rest[0] = match &self.ty {
            ValueType::Switch => 0,
            ValueType::Pwm => 1,
        };

        Ok(&mut rest[1..])
    }

    fn deserialize(buffer: &'r [u8]) -> Result<(Self, &'r [u8]), Self::DeSerError> {
        let (name, rest) = Sendable::deserialize(buffer)?;
        let ty = match rest[0] {
            0 => ValueType::Switch,
            1 => ValueType::Pwm,
            _ => todo!(),
        };

        Ok((Self { name, ty }, &rest[1..]))
    }
}

/// An Iterator for Data being send or received, allowing for lists in the Packets
#[derive(Debug, PartialEq, Eq, Clone)]
pub enum OptionsIter<'r, T> {
    Received { buffer: &'r [u8], length: usize },
    Fixed { data: &'r [T], index: usize },
}

impl<'r, T> OptionsIter<'r, T> {
    /// Get the number of Elements in the remaining Iterator
    pub fn length(&self) -> usize {
        match self {
            Self::Received { length, .. } => *length,
            Self::Fixed { data, .. } => data.len(),
        }
    }
}

impl<'r, T> From<&'r [T]> for OptionsIter<'r, T> {
    fn from(raw: &'r [T]) -> Self {
        Self::Fixed {
            data: raw,
            index: 0,
        }
    }
}
impl<'r, const N: usize, T> From<&'r [T; N]> for OptionsIter<'r, T> {
    fn from(raw: &'r [T; N]) -> Self {
        Self::from(raw as &'r [T])
    }
}

impl<'r, T> Iterator for OptionsIter<'r, T>
where
    T: Clone + Sendable<'r>,
{
    type Item = T;

    fn next(&mut self) -> Option<Self::Item> {
        match self {
            Self::Received { buffer, length } => {
                if *length == 0 {
                    return None;
                }

                let (value, rest) = T::deserialize(buffer).ok()?;

                *length -= 1;
                *buffer = rest;

                Some(value)
            }
            Self::Fixed { data, index } => {
                let resp = data.get(*index).cloned();
                *index = index.saturating_add(1);
                resp
            }
        }
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OptionsIterDeserializeError<E> {
    EmptyBuffer,
    InnerError(E),
}
impl<E> From<E> for OptionsIterDeserializeError<E> {
    fn from(e: E) -> Self {
        Self::InnerError(e)
    }
}

#[derive(Debug, PartialEq, Eq)]
pub enum OptionsIterSerializeError<E> {
    EmptyBuffer,
    InnerError(E),
}
impl<E> From<E> for OptionsIterSerializeError<E> {
    fn from(e: E) -> Self {
        Self::InnerError(e)
    }
}

impl<'r, T> Sendable<'r> for OptionsIter<'r, T>
where
    T: Sendable<'r>,
{
    type SerError = OptionsIterSerializeError<T::SerError>;
    type DeSerError = OptionsIterDeserializeError<T::DeSerError>;

    fn serialize<'b>(&self, mut buffer: &'b mut [u8]) -> Result<&'b mut [u8], Self::SerError> {
        if buffer.is_empty() {
            return Err(OptionsIterSerializeError::EmptyBuffer);
        }

        match self {
            Self::Fixed { data, .. } => {
                buffer[0] = data.len() as u8;

                buffer = &mut buffer[1..];
                for item in data.iter() {
                    buffer = item.serialize(buffer)?;
                }

                Ok(buffer)
            }
            Self::Received {
                buffer: r_buf,
                length,
            } => {
                buffer[0] = *length as u8;

                let r_length = r_buf.len();
                buffer[1..r_length + 1].copy_from_slice(&r_buf[..r_length]);

                Ok(&mut buffer[r_length + 1..])
            }
        }
    }

    fn deserialize(buffer: &'r [u8]) -> Result<(Self, &'r [u8]), Self::DeSerError> {
        if buffer.is_empty() {
            return Err(OptionsIterDeserializeError::EmptyBuffer);
        }

        let items = buffer[0] as usize;

        let mut length = 0;
        let mut rest = &buffer[1..];
        for _ in 0..items {
            let (_, tmp): (T, _) = Sendable::deserialize(rest)?;

            length += rest.len() - tmp.len();
            rest = tmp;
        }

        Ok((
            Self::Received {
                buffer: &buffer[1..1 + length],
                length: items,
            },
            rest,
        ))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn optioniter_serialize_deserialize() {
        let fixed_iter: OptionsIter<'static, ConfigOption> = (&[
            ConfigOption {
                name: "testing1",
                ty: ValueType::Pwm,
            },
            ConfigOption {
                name: "testing2",
                ty: ValueType::Switch,
            },
        ])
            .into();

        let mut buffer = [0; 256];

        fixed_iter.serialize(&mut buffer).expect("Should work");

        let (deserialized, _): (OptionsIter<'_, ConfigOption>, _) =
            Sendable::deserialize(&buffer).expect("Should work");

        assert_eq!(fixed_iter.length(), deserialized.length());
        assert!(fixed_iter
            .zip(deserialized)
            .all(|(first, second)| first == second));
    }

    #[test]
    fn optioniter_serialize_deserialize_serialize() {
        let fixed_iter: OptionsIter<'static, ConfigOption> = (&[
            ConfigOption {
                name: "testing1",
                ty: ValueType::Pwm,
            },
            ConfigOption {
                name: "testing2",
                ty: ValueType::Switch,
            },
        ])
            .into();

        let mut buffer = [0; 256];

        fixed_iter.serialize(&mut buffer).expect("Should work");

        let (deserialized, _): (OptionsIter<'_, ConfigOption>, _) =
            Sendable::deserialize(&buffer).expect("Should work");

        assert_eq!(fixed_iter.length(), deserialized.length());

        let mut buffer2 = [0; 256];
        deserialized.serialize(&mut buffer2).expect("Should work");

        assert_eq!(buffer, buffer2);
    }
}
