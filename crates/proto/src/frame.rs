//! Quadros: u32 (LE) com o tamanho, seguido dos bytes.

use std::io::{Read, Write};

use serde::{Serialize, de::DeserializeOwned};

use crate::{Error, Result};

pub const MAX_FRAME: usize = 32 * 1024 * 1024;

pub fn write_frame(w: &mut impl Write, data: &[u8]) -> Result<()> {
    if data.len() > MAX_FRAME {
        return Err(Error::Other("quadro grande demais".into()));
    }
    w.write_all(&(data.len() as u32).to_le_bytes())?;
    w.write_all(data)?;
    w.flush()?;
    Ok(())
}

pub fn read_frame(r: &mut impl Read) -> Result<Vec<u8>> {
    let mut len = [0u8; 4];
    r.read_exact(&mut len)?;
    let n = u32::from_le_bytes(len) as usize;
    if n > MAX_FRAME {
        return Err(Error::Other("quadro grande demais".into()));
    }
    let mut buf = vec![0u8; n];
    r.read_exact(&mut buf)?;
    Ok(buf)
}

pub fn send<T: Serialize>(w: &mut impl Write, msg: &T) -> Result<()> {
    write_frame(w, &postcard::to_stdvec(msg)?)
}

pub fn recv<T: DeserializeOwned>(r: &mut impl Read) -> Result<T> {
    Ok(postcard::from_bytes(&read_frame(r)?)?)
}
