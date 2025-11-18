//! SDP (Session Description Protocol) generation for AES67 blocks.

use gstreamer as gst;
use strom_types::{BlockInstance, PropertyValue};

/// Extract audio format details from GStreamer caps.
/// Returns (sample_rate, channels) or None if the caps don't contain audio info.
pub fn parse_audio_caps(caps: &gst::Caps) -> Option<(i32, i32)> {
    // Get the first structure from the caps
    let structure = caps.structure(0)?;

    // Check if it's audio caps
    let name = structure.name();
    if !name.starts_with("audio/") {
        return None;
    }

    // Extract sample rate (field name is "rate")
    let sample_rate = structure.get::<i32>("rate").ok()?;

    // Extract channels
    let channels = structure.get::<i32>("channels").ok()?;

    Some((sample_rate, channels))
}

/// Check if an IP address is multicast (224.0.0.0 to 239.255.255.255).
fn is_multicast_address(addr: &str) -> bool {
    // Try to parse as IPv4 address
    let parts: Vec<&str> = addr.split('.').collect();
    if parts.len() != 4 {
        return false;
    }

    // Parse first octet
    if let Ok(first_octet) = parts[0].parse::<u8>() {
        // Multicast range is 224-239 (class D)
        (224..=239).contains(&first_octet)
    } else {
        false
    }
}

/// Generate SDP for an AES67 output block instance.
///
/// The SDP describes the RTP stream parameters that receivers need to connect.
/// Uses configured block properties for accurate stream description.
pub fn generate_aes67_output_sdp(
    block: &BlockInstance,
    session_name: &str,
    sample_rate: Option<i32>,
    channels: Option<i32>,
) -> String {
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

    // Get bit depth to determine L16 vs L24
    let bit_depth = block
        .properties
        .get("bit_depth")
        .and_then(|v| match v {
            PropertyValue::Int(i) => Some(*i as i32),
            PropertyValue::String(s) => s.parse::<i32>().ok(),
            _ => None,
        })
        .unwrap_or(24);

    // Get packet time (ptime) in milliseconds
    let ptime = block
        .properties
        .get("ptime")
        .and_then(|v| match v {
            PropertyValue::Float(f) => Some(*f),
            PropertyValue::String(s) => s.parse::<f64>().ok(),
            _ => None,
        })
        .unwrap_or(1.0);

    // Get local IP for origin field (o=)
    // In a real implementation, you'd detect the actual network interface IP
    let origin_ip = "127.0.0.1";

    // Session ID and version (using timestamp)
    let session_id = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .unwrap()
        .as_secs();

    // Use provided values or fall back to AES67 defaults: 48kHz, 2 channels
    let sample_rate = sample_rate.unwrap_or(48000);
    let channels = channels.unwrap_or(2);
    let payload_type = 96; // Dynamic payload type

    // Determine encoding name based on bit depth
    let encoding = match bit_depth {
        16 => "L16",
        24 => "L24",
        _ => "L24", // Default to 24-bit
    };

    // Check if the host is a multicast address and format connection line accordingly
    // Multicast IPv4 addresses are in range 224.0.0.0 to 239.255.255.255
    let connection_line = if is_multicast_address(host) {
        format!("c=IN IP4 {}/32", host) // Add TTL for multicast
    } else {
        format!("c=IN IP4 {}", host) // No TTL for unicast
    };

    // Generate SDP
    format!(
        "v=0\r
o=- {} {} IN IP4 {}\r
s={}\r
{}\r
t=0 0\r
a=recvonly\r
m=audio {} RTP/AVP {}\r
a=rtpmap:{} {}/{}/{}\r
a=ptime:{}\r
a=ts-refclk:ptp=IEEE1588-2008:00-00-00-00-00-00-00-00:0\r
a=mediaclk:direct=0\r
",
        session_id,
        session_id,
        origin_ip,
        session_name,
        connection_line,
        port,
        payload_type,
        payload_type,
        encoding,
        sample_rate,
        channels,
        ptime
    )
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::collections::HashMap;

    #[test]
    fn test_parse_audio_caps_44100_mono() {
        gst::init().unwrap();

        // Create caps for 44.1kHz mono audio (audiotestsrc default)
        let caps = gst::Caps::builder("audio/x-raw")
            .field("rate", 44100i32)
            .field("channels", 1i32)
            .build();

        let result = parse_audio_caps(&caps);
        assert_eq!(result, Some((44100, 1)));
    }

    #[test]
    fn test_parse_audio_caps_48000_stereo() {
        gst::init().unwrap();

        // Create caps for 48kHz stereo (AES67 standard)
        let caps = gst::Caps::builder("audio/x-raw")
            .field("rate", 48000i32)
            .field("channels", 2i32)
            .build();

        let result = parse_audio_caps(&caps);
        assert_eq!(result, Some((48000, 2)));
    }

    #[test]
    fn test_parse_audio_caps_non_audio() {
        gst::init().unwrap();

        // Create video caps - should return None
        let caps = gst::Caps::builder("video/x-raw")
            .field("width", 1920i32)
            .field("height", 1080i32)
            .build();

        let result = parse_audio_caps(&caps);
        assert_eq!(result, None);
    }

    #[test]
    fn test_generate_sdp_default_values() {
        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties: HashMap::new(),
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Test Stream", None, None);

        assert!(sdp.contains("s=Test Stream"));
        assert!(sdp.contains("c=IN IP4 239.69.1.1/32")); // Multicast should have /32 TTL
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
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Custom Stream", None, None);

        assert!(sdp.contains("s=Custom Stream"));
        assert!(sdp.contains("c=IN IP4 239.1.2.3/32")); // Multicast should have /32 TTL
        assert!(sdp.contains("m=audio 6000 RTP/AVP 96"));
    }

    #[test]
    fn test_generate_sdp_with_44100_mono() {
        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties: HashMap::new(),
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        // Test with audiotestsrc defaults: 44.1kHz mono
        let sdp = generate_aes67_output_sdp(&block, "Test Stream", Some(44100), Some(1));

        assert!(sdp.contains("s=Test Stream"));
        assert!(sdp.contains("a=rtpmap:96 L24/44100/1"));
    }

    #[test]
    fn test_generate_sdp_with_string_bit_depth_16() {
        let mut properties = HashMap::new();
        properties.insert(
            "bit_depth".to_string(),
            PropertyValue::String("16".to_string()),
        );

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Test Stream", None, None);

        // Should use L16 encoding, not L24
        assert!(sdp.contains("a=rtpmap:96 L16/48000/2"));
        assert!(!sdp.contains("L24"));
    }

    #[test]
    fn test_generate_sdp_with_string_bit_depth_24() {
        let mut properties = HashMap::new();
        properties.insert(
            "bit_depth".to_string(),
            PropertyValue::String("24".to_string()),
        );

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Test Stream", None, None);

        // Should use L24 encoding
        assert!(sdp.contains("a=rtpmap:96 L24/48000/2"));
    }

    #[test]
    fn test_generate_sdp_with_string_ptime() {
        let mut properties = HashMap::new();
        properties.insert(
            "ptime".to_string(),
            PropertyValue::String("4.0".to_string()),
        );

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Test Stream", None, None);

        // Should have ptime=4.0, not 1.0
        assert!(sdp.contains("a=ptime:4"));
        assert!(!sdp.contains("a=ptime:1"));
    }

    #[test]
    fn test_generate_sdp_with_all_string_properties() {
        let mut properties = HashMap::new();
        properties.insert(
            "bit_depth".to_string(),
            PropertyValue::String("16".to_string()),
        );
        properties.insert(
            "sample_rate".to_string(),
            PropertyValue::String("96000".to_string()),
        );
        properties.insert(
            "ptime".to_string(),
            PropertyValue::String("0.125".to_string()),
        );
        properties.insert("channels".to_string(), PropertyValue::Int(8));
        properties.insert(
            "host".to_string(),
            PropertyValue::String("239.1.2.3".to_string()),
        );
        properties.insert("port".to_string(), PropertyValue::Int(5008));

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Multi-channel Stream", Some(96000), Some(8));

        // Verify all properties are correctly reflected in SDP
        assert!(sdp.contains("s=Multi-channel Stream"));
        assert!(sdp.contains("c=IN IP4 239.1.2.3/32")); // Multicast should have /32 TTL
        assert!(sdp.contains("m=audio 5008 RTP/AVP 96"));
        assert!(sdp.contains("a=rtpmap:96 L16/96000/8")); // L16 for 16-bit
        assert!(sdp.contains("a=ptime:0.125")); // 0.125 ms ptime
    }

    #[test]
    fn test_multicast_address_detection() {
        // Test multicast addresses (224.0.0.0 to 239.255.255.255)
        assert!(is_multicast_address("224.0.0.0"));
        assert!(is_multicast_address("239.255.255.255"));
        assert!(is_multicast_address("239.69.11.44"));
        assert!(is_multicast_address("225.1.2.3"));

        // Test non-multicast addresses
        assert!(!is_multicast_address("127.0.0.1"));
        assert!(!is_multicast_address("192.168.1.1"));
        assert!(!is_multicast_address("10.0.0.1"));
        assert!(!is_multicast_address("223.255.255.255")); // Just below multicast range
        assert!(!is_multicast_address("240.0.0.0")); // Just above multicast range

        // Test invalid addresses
        assert!(!is_multicast_address("invalid"));
        assert!(!is_multicast_address("999.999.999.999"));
        assert!(!is_multicast_address(""));
    }

    #[test]
    fn test_generate_sdp_unicast_no_ttl() {
        let mut properties = HashMap::new();
        properties.insert(
            "host".to_string(),
            PropertyValue::String("192.168.1.100".to_string()),
        );

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Unicast Stream", None, None);

        // Unicast addresses should NOT have /TTL suffix
        assert!(sdp.contains("c=IN IP4 192.168.1.100\r"));
        assert!(!sdp.contains("/32"));
    }

    #[test]
    fn test_generate_sdp_multicast_has_ttl() {
        let mut properties = HashMap::new();
        properties.insert(
            "host".to_string(),
            PropertyValue::String("239.69.11.44".to_string()),
        );

        let block = BlockInstance {
            id: "block_0".to_string(),
            block_definition_id: "builtin.aes67_output".to_string(),
            name: None,
            properties,
            position: strom_types::block::Position { x: 0.0, y: 0.0 },
            runtime_data: None,
        };

        let sdp = generate_aes67_output_sdp(&block, "Multicast Stream", None, None);

        // Multicast addresses MUST have /32 TTL suffix
        assert!(sdp.contains("c=IN IP4 239.69.11.44/32\r"));
    }
}
