//! SDP (Session Description Protocol) generation for AES67 blocks.

use strom_types::{BlockInstance, PropertyValue};

/// Generate SDP for an AES67 output block instance.
///
/// The SDP describes the RTP stream parameters that receivers need to connect.
pub fn generate_aes67_output_sdp(block: &BlockInstance, session_name: &str) -> String {
    // Extract properties or use defaults
    let host = block
        .properties
        .get("host")
        .and_then(|v| {
            if let PropertyValue::String(s) = v {
                Some(s.as_str())
            } else {
                None
            }
        })
        .unwrap_or("239.69.1.1");

    let port = block
        .properties
        .get("port")
        .and_then(|v| {
            if let PropertyValue::Int(i) = v {
                Some(*i)
            } else {
                None
            }
        })
        .unwrap_or(5004);

    // Get local IP for origin field (o=)
    // In a real implementation, you'd detect the actual network interface IP
    let origin_ip = "127.0.0.1";

    // Session ID and version (using timestamp)
    let session_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // AES67 uses L24 (24-bit linear PCM) RTP payload
    // Standard AES67 profile: 48kHz sample rate, 2 channels
    let sample_rate = 48000;
    let channels = 2;
    let payload_type = 96; // Dynamic payload type for L24

    // Generate SDP
    format!(
        "v=0\r
o=- {} {} IN IP4 {}\r
s={}\r
c=IN IP4 {}\r
t=0 0\r
a=recvonly\r
m=audio {} RTP/AVP {}\r
a=rtpmap:{} L24/{}/{}\r
a=ptime:1\r
a=ts-refclk:ptp=IEEE1588-2008:00-00-00-00-00-00-00-00:0\r
a=mediaclk:direct=0\r
",
        session_id,
        session_id,
        origin_ip,
        session_name,
        host,
        port,
        payload_type,
        payload_type,
        sample_rate,
        channels
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_generate_sdp_default_values() {
        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties: HashMap::new(),
            position: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Test Stream");

        assert!(sdp.contains("s=Test Stream"));
        assert!(sdp.contains("c=IN IP4 239.69.1.1"));
        assert!(sdp.contains("m=audio 5004 RTP/AVP 96"));
        assert!(sdp.contains("a=rtpmap:96 L24/48000/2"));
    }

    #[test]
    fn test_generate_sdp_custom_values() {
        let mut properties = HashMap::new();
        properties.insert(
            "host".to_string(),
            PropertyValue::String("239.1.2.3".to_string()),
        );
        properties.insert("port".to_string(), PropertyValue::Int(6000));

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Custom Stream");

        assert!(sdp.contains("s=Custom Stream"));
        assert!(sdp.contains("c=IN IP4 239.1.2.3"));
        assert!(sdp.contains("m=audio 6000 RTP/AVP 96"));
    }
}
