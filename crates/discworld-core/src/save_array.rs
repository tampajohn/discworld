// SPDX-License-Identifier: GPL-3.0-or-later
//! Fixed-size snapshot arrays, decoded with a compile-time allocation bound.
use serde::{Deserialize, Deserializer, Serialize, Serializer, de, ser::SerializeTuple};

pub fn serialize<T: Serialize, S: Serializer, const N: usize>(
    values: &[T; N],
    serializer: S,
) -> Result<S::Ok, S::Error> {
    let mut tuple = serializer.serialize_tuple(N)?;
    for value in values {
        tuple.serialize_element(value)?;
    }
    tuple.end()
}

pub fn deserialize<'de, T: Deserialize<'de>, D: Deserializer<'de>, const N: usize>(
    deserializer: D,
) -> Result<[T; N], D::Error> {
    struct ArrayVisitor<T, const N: usize>(std::marker::PhantomData<T>);
    impl<'de, T: Deserialize<'de>, const N: usize> de::Visitor<'de> for ArrayVisitor<T, N> {
        type Value = [T; N];
        fn expecting(&self, f: &mut std::fmt::Formatter) -> std::fmt::Result {
            write!(f, "an array of {N} elements")
        }
        fn visit_seq<A: de::SeqAccess<'de>>(self, mut seq: A) -> Result<Self::Value, A::Error> {
            let mut values = Vec::with_capacity(N);
            for index in 0..N {
                values.push(
                    seq.next_element()?
                        .ok_or_else(|| de::Error::invalid_length(index, &self))?,
                );
            }
            values
                .try_into()
                .map_err(|_| de::Error::custom("fixed snapshot array length"))
        }
    }
    deserializer.deserialize_tuple(N, ArrayVisitor(std::marker::PhantomData))
}
