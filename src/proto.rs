// Copyright 2026 Google LLC
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     https://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.

//! Zero-dependency Varint and Protobuf encoder/decoder for localharness configurations.
//!
//! Exposes encoders for `InputConfig` and decoders for `OutputConfig`.

#[derive(Debug, Default, Clone)]
pub struct OutputConfig {
    pub port: i32,
    pub api_key: String,
}

/// Encodes `InputConfig` containing only `storage_directory` to Protobuf binary bytes.
pub fn encode_input_config(storage_directory: &str) -> Vec<u8> {
    let mut buf = Vec::new();
    if !storage_directory.is_empty() {
        // Field 1, wire type 2 (length-delimited string): tag = (1 << 3) | 2 = 10 (0x0A)
        buf.push(0x0A);
        let bytes = storage_directory.as_bytes();
        encode_varint(bytes.len() as u64, &mut buf);
        buf.extend_from_slice(bytes);
    }
    buf
}

/// Decodes binary bytes into `OutputConfig`.
pub fn decode_output_config(mut data: &[u8]) -> Result<OutputConfig, String> {
    let mut config = OutputConfig::default();
    while !data.is_empty() {
        let tag = decode_varint(&mut data)?;
        let field_num = tag >> 3;
        let wire_type = tag & 7;
        match field_num {
            1 => {
                if wire_type != 0 {
                    return Err(format!("Expected wire type 0 for port, got {}", wire_type));
                }
                let val = decode_varint(&mut data)?;
                config.port = val as i32;
            }
            2 => {
                if wire_type != 2 {
                    return Err(format!(
                        "Expected wire type 2 for api_key, got {}",
                        wire_type
                    ));
                }
                let len = decode_varint(&mut data)? as usize;
                if data.len() < len {
                    return Err("Unexpected EOF reading api_key".to_string());
                }
                let key_bytes = &data[..len];
                data = &data[len..];
                config.api_key = String::from_utf8(key_bytes.to_vec())
                    .map_err(|e| format!("Invalid UTF-8 in api_key: {}", e))?;
            }
            _ => {
                skip_field(wire_type, &mut data)?;
            }
        }
    }
    Ok(config)
}

fn encode_varint(mut value: u64, buf: &mut Vec<u8>) {
    while value >= 0x80 {
        buf.push(((value & 0x7F) | 0x80) as u8);
        value >>= 7;
    }
    buf.push(value as u8);
}

fn decode_varint(data: &mut &[u8]) -> Result<u64, String> {
    let mut value = 0u64;
    let mut shift = 0;
    loop {
        if data.is_empty() {
            return Err("Unexpected EOF reading varint".to_string());
        }
        let byte = data[0];
        *data = &data[1..];
        value |= ((byte & 0x7F) as u64) << shift;
        if (byte & 0x80) == 0 {
            break;
        }
        shift += 7;
        if shift >= 64 {
            return Err("Varint overflow".to_string());
        }
    }
    Ok(value)
}

fn skip_field(wire_type: u64, data: &mut &[u8]) -> Result<(), String> {
    match wire_type {
        0 => {
            decode_varint(data)?;
        }
        1 => {
            if data.len() < 8 {
                return Err("Unexpected EOF skipping 64-bit field".to_string());
            }
            *data = &data[8..];
        }
        2 => {
            let len = decode_varint(data)? as usize;
            if data.len() < len {
                return Err("Unexpected EOF skipping length-delimited field".to_string());
            }
            *data = &data[len..];
        }
        5 => {
            if data.len() < 4 {
                return Err("Unexpected EOF skipping 32-bit field".to_string());
            }
            *data = &data[4..];
        }
        _ => return Err(format!("Unsupported wire type: {}", wire_type)),
    }
    Ok(())
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_encode_decode() {
        let storage_dir = "/tmp/antigravity";
        let encoded = encode_input_config(storage_dir);
        assert_eq!(encoded[0], 0x0A);
        assert_eq!(encoded[1], storage_dir.len() as u8);
        assert_eq!(&encoded[2..], storage_dir.as_bytes());

        // Simple manual output config bytes:
        // port (field 1, varint 8080 = 0x90 0x3F): tag = 8 (0x08), value = 0x90 0x3F
        // api_key (field 2, string "secret"): tag = 18 (0x12), len = 6, value = "secret"
        let out_bytes = vec![
            0x08, 0x90, 0x3F, 0x12, 0x06, b's', b'e', b'c', b'r', b'e', b't',
        ];
        let decoded = decode_output_config(&out_bytes).unwrap();
        assert_eq!(decoded.port, 8080);
        assert_eq!(decoded.api_key, "secret");
    }
}
