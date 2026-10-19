#![allow(dead_code)]

use std::io;
use std::io::ErrorKind;
use std::io::Read;
use std::str;

use crate::four_cc::FourCC;

#[allow(unused)]
#[non_exhaustive]
pub(crate) struct BinaryReader {}

impl BinaryReader {
    pub(crate) fn read_i8<R: Read>(reader: &mut R) -> Result<i8, io::Error> {
        let mut data: [u8; 1] = [0; 1];
        reader.read_exact(&mut data)?;
        Ok(i8::from_le_bytes(data))
    }

    pub(crate) fn read_u8<R: Read>(reader: &mut R) -> Result<u8, io::Error> {
        let mut data: [u8; 1] = [0; 1];
        reader.read_exact(&mut data)?;
        Ok(u8::from_le_bytes(data))
    }

    pub(crate) fn read_i16<R: Read>(reader: &mut R) -> Result<i16, io::Error> {
        let mut data: [u8; 2] = [0; 2];
        reader.read_exact(&mut data)?;
        Ok(i16::from_le_bytes(data))
    }

    pub(crate) fn read_u16<R: Read>(reader: &mut R) -> Result<u16, io::Error> {
        let mut data: [u8; 2] = [0; 2];
        reader.read_exact(&mut data)?;
        Ok(u16::from_le_bytes(data))
    }

    pub(crate) fn read_i32<R: Read>(reader: &mut R) -> Result<i32, io::Error> {
        let mut data: [u8; 4] = [0; 4];
        reader.read_exact(&mut data)?;
        Ok(i32::from_le_bytes(data))
    }

    pub(crate) fn read_u32<R: Read>(reader: &mut R) -> Result<u32, io::Error> {
        let mut data: [u8; 4] = [0; 4];
        reader.read_exact(&mut data)?;
        Ok(u32::from_le_bytes(data))
    }

    pub(crate) fn read_i16_big_endian<R: Read>(reader: &mut R) -> Result<i16, io::Error> {
        let mut data: [u8; 2] = [0; 2];
        reader.read_exact(&mut data)?;
        Ok(i16::from_be_bytes(data))
    }

    pub(crate) fn read_i32_big_endian<R: Read>(reader: &mut R) -> Result<i32, io::Error> {
        let mut data: [u8; 4] = [0; 4];
        reader.read_exact(&mut data)?;
        Ok(i32::from_be_bytes(data))
    }

    pub(crate) fn read_i32_variable_length<R: Read>(reader: &mut R) -> Result<i32, io::Error> {
        let mut acc: i32 = 0;
        let mut count: i32 = 0;

        loop {
            let value = BinaryReader::read_u8(reader)? as i32;
            acc = (acc << 7) | (value & 127);
            if (value & 128) == 0 {
                break;
            }
            count += 1;
            if count == 4 {
                return Err(io::Error::new(
                    ErrorKind::InvalidData,
                    "the length of the value must be equal to or less than 4",
                ));
            }
        }

        Ok(acc)
    }

    pub(crate) fn read_four_cc<R: Read>(reader: &mut R) -> Result<FourCC, io::Error> {
        let mut data: [u8; 4] = [0; 4];
        reader.read_exact(&mut data)?;
        Ok(FourCC::from_bytes(data))
    }

    pub(crate) fn read_fixed_length_string<R: Read>(
        reader: &mut R,
        length: usize,
    ) -> Result<String, io::Error> {
        let mut data: Vec<u8> = vec![0; length];
        reader.read_exact(&mut data)?;

        let mut actual_length: usize = 0;
        for value in &mut data {
            if *value == 0 {
                break;
            }
            actual_length += 1;
        }

        // Replace non-ASCII characters with '?'.
        // Tabs and returns are preserved.
        for value in &mut data[0..actual_length] {
            if !(9..=126).contains(value) {
                *value = 63; // '?'
            }
        }

        Ok(str::from_utf8(&data[0..actual_length]).unwrap().to_string())
    }

    pub(crate) fn discard_data<R: Read>(reader: &mut R, size: usize) -> Result<(), io::Error> {
        let mut data: Vec<u8> = vec![0; size];
        reader.read_exact(&mut data)
    }

    pub(crate) fn read_wave_data<R: Read>(
        reader: &mut R,
        size: usize,
    ) -> Result<Vec<i16>, io::Error> {
        if size % 2 != 0 {
            return Err(io::Error::new(
                ErrorKind::InvalidData,
                "odd PCM byte length",
            ));
        }
        let mut data = vec![0u8; size];
        reader.read_exact(&mut data)?;
        let samples = data
            .chunks_exact(2)
            .map(|b| i16::from_le_bytes([b[0], b[1]]))
            .collect();

        Ok(samples)
    }
}

#[cfg(test)]
mod safe_pcm_tests {
    use super::*;
    #[test]
    fn little_endian_and_odd_length() {
        let pcm = BinaryReader::read_wave_data(&mut &b"\x00\x80\xff\x7f"[..], 4).unwrap();
        assert_eq!(pcm, [-32768, 32767]);
        assert!(BinaryReader::read_wave_data(&mut &b"\x00\x00\x00"[..], 3).is_err());
        assert!(BinaryReader::read_wave_data(&mut &b"\x00"[..], 2).is_err());
    }
}
