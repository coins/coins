/// Invoice / Payment Request module
///
/// Provides a `coins://` URI format for payment requests.
/// Format: `coins://pay?addr=<hex>&amount=<u32>&token=<u16>&memo=<string>&expires=<unix_ts>`

use serde::{Serialize, Deserialize};

/// A payment request / invoice
#[derive(Debug, Clone, Serialize, Deserialize, PartialEq)]
pub struct Invoice {
    /// Recipient address (hex-encoded public key) - required
    pub addr: String,
    /// Requested amount in sats - optional
    pub amount: Option<u32>,
    /// Token ID - optional (defaults to 0 / native)
    pub token_id: Option<u16>,
    /// Human-readable memo/description - optional
    pub memo: Option<String>,
    /// Expiration Unix timestamp - optional
    pub expires: Option<u64>,
}

/// Error type for invoice parsing
#[derive(Debug, Clone, PartialEq)]
pub enum InvoiceError {
    /// URI doesn't start with "coins://pay"
    InvalidScheme,
    /// Missing required address field
    MissingAddress,
    /// Invalid address format (not valid hex)
    InvalidAddress,
    /// Invalid amount value
    InvalidAmount,
    /// Invalid token ID value
    InvalidTokenId,
    /// Invalid expiration timestamp
    InvalidExpires,
    /// Malformed URI
    MalformedUri,
}

impl std::fmt::Display for InvoiceError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        match self {
            InvoiceError::InvalidScheme => write!(f, "URI must start with 'coins://pay'"),
            InvoiceError::MissingAddress => write!(f, "Missing required 'addr' parameter"),
            InvoiceError::InvalidAddress => write!(f, "Invalid address: must be a hex string"),
            InvoiceError::InvalidAmount => write!(f, "Invalid amount: must be a positive integer"),
            InvoiceError::InvalidTokenId => write!(f, "Invalid token ID: must be a non-negative integer"),
            InvoiceError::InvalidExpires => write!(f, "Invalid expiration: must be a Unix timestamp"),
            InvoiceError::MalformedUri => write!(f, "Malformed URI"),
        }
    }
}

impl std::error::Error for InvoiceError {}

impl Invoice {
    /// Create a new invoice with just an address (minimum required field)
    pub fn new(addr: String) -> Self {
        Invoice {
            addr,
            amount: None,
            token_id: None,
            memo: None,
            expires: None,
        }
    }

    /// Serialize the invoice to a `coins://` URI string
    pub fn to_uri(&self) -> String {
        let mut params = vec![format!("addr={}", self.addr)];

        if let Some(amount) = self.amount {
            params.push(format!("amount={}", amount));
        }
        if let Some(token_id) = self.token_id {
            params.push(format!("token={}", token_id));
        }
        if let Some(ref memo) = self.memo {
            params.push(format!("memo={}", url_encode(memo)));
        }
        if let Some(expires) = self.expires {
            params.push(format!("expires={}", expires));
        }

        format!("coins://pay?{}", params.join("&"))
    }

    /// Parse a `coins://` URI string into an Invoice
    pub fn from_uri(uri: &str) -> Result<Self, InvoiceError> {
        // Must start with coins://pay
        let rest = uri.strip_prefix("coins://pay")
            .ok_or(InvoiceError::InvalidScheme)?;

        // Parse query string
        let query = rest.strip_prefix('?')
            .ok_or(InvoiceError::MissingAddress)?;

        let mut addr = None;
        let mut amount = None;
        let mut token_id = None;
        let mut memo = None;
        let mut expires = None;

        for pair in query.split('&') {
            if pair.is_empty() {
                continue;
            }
            let (key, value) = pair.split_once('=')
                .ok_or(InvoiceError::MalformedUri)?;

            match key {
                "addr" => {
                    // Validate hex
                    if value.is_empty() || !value.chars().all(|c| c.is_ascii_hexdigit()) {
                        return Err(InvoiceError::InvalidAddress);
                    }
                    addr = Some(value.to_string());
                }
                "amount" => {
                    amount = Some(value.parse::<u32>().map_err(|_| InvoiceError::InvalidAmount)?);
                }
                "token" => {
                    token_id = Some(value.parse::<u16>().map_err(|_| InvoiceError::InvalidTokenId)?);
                }
                "memo" => {
                    memo = Some(url_decode(value));
                }
                "expires" => {
                    expires = Some(value.parse::<u64>().map_err(|_| InvoiceError::InvalidExpires)?);
                }
                _ => {
                    // Ignore unknown parameters for forward compatibility
                }
            }
        }

        let addr = addr.ok_or(InvoiceError::MissingAddress)?;

        Ok(Invoice {
            addr,
            amount,
            token_id,
            memo,
            expires,
        })
    }

    /// Check if the invoice has expired
    pub fn is_expired(&self) -> bool {
        if let Some(expires) = self.expires {
            let now = std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .map(|d| d.as_secs())
                .unwrap_or(0);
            now > expires
        } else {
            false
        }
    }
}

/// Simple URL encoding for memo field
fn url_encode(s: &str) -> String {
    let mut encoded = String::with_capacity(s.len());
    for c in s.chars() {
        match c {
            'A'..='Z' | 'a'..='z' | '0'..='9' | '-' | '_' | '.' | '~' => {
                encoded.push(c);
            }
            ' ' => encoded.push_str("%20"),
            _ => {
                for byte in c.to_string().as_bytes() {
                    encoded.push_str(&format!("%{:02X}", byte));
                }
            }
        }
    }
    encoded
}

/// Simple URL decoding for memo field
fn url_decode(s: &str) -> String {
    let mut decoded = String::with_capacity(s.len());
    let mut chars = s.chars();
    while let Some(c) = chars.next() {
        if c == '%' {
            let hex: String = chars.by_ref().take(2).collect();
            if hex.len() == 2 {
                if let Ok(byte) = u8::from_str_radix(&hex, 16) {
                    decoded.push(byte as char);
                } else {
                    decoded.push('%');
                    decoded.push_str(&hex);
                }
            }
        } else if c == '+' {
            decoded.push(' ');
        } else {
            decoded.push(c);
        }
    }
    decoded
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_invoice_roundtrip_minimal() {
        let inv = Invoice::new("abcdef1234567890".to_string());
        let uri = inv.to_uri();
        let parsed = Invoice::from_uri(&uri).unwrap();
        assert_eq!(inv, parsed);
    }

    #[test]
    fn test_invoice_roundtrip_full() {
        let inv = Invoice {
            addr: "abcdef1234567890abcdef1234567890abcdef1234567890abcdef1234567890".to_string(),
            amount: Some(1000),
            token_id: Some(0),
            memo: Some("Coffee payment".to_string()),
            expires: Some(1738600000),
        };
        let uri = inv.to_uri();
        let parsed = Invoice::from_uri(&uri).unwrap();
        assert_eq!(inv, parsed);
    }

    #[test]
    fn test_uri_generation() {
        let inv = Invoice {
            addr: "abc123".to_string(),
            amount: Some(100),
            token_id: None,
            memo: Some("Coffee".to_string()),
            expires: None,
        };
        let uri = inv.to_uri();
        assert_eq!(uri, "coins://pay?addr=abc123&amount=100&memo=Coffee");
    }

    #[test]
    fn test_parse_minimal_uri() {
        let inv = Invoice::from_uri("coins://pay?addr=abc123").unwrap();
        assert_eq!(inv.addr, "abc123");
        assert_eq!(inv.amount, None);
        assert_eq!(inv.token_id, None);
        assert_eq!(inv.memo, None);
        assert_eq!(inv.expires, None);
    }

    #[test]
    fn test_parse_full_uri() {
        let inv = Invoice::from_uri("coins://pay?addr=abc123&amount=100&token=0&memo=Coffee&expires=1738600000").unwrap();
        assert_eq!(inv.addr, "abc123");
        assert_eq!(inv.amount, Some(100));
        assert_eq!(inv.token_id, Some(0));
        assert_eq!(inv.memo, Some("Coffee".to_string()));
        assert_eq!(inv.expires, Some(1738600000));
    }

    #[test]
    fn test_invalid_scheme() {
        assert_eq!(Invoice::from_uri("http://pay?addr=abc123"), Err(InvoiceError::InvalidScheme));
    }

    #[test]
    fn test_missing_address() {
        assert_eq!(Invoice::from_uri("coins://pay?amount=100"), Err(InvoiceError::MissingAddress));
    }

    #[test]
    fn test_invalid_address() {
        assert_eq!(Invoice::from_uri("coins://pay?addr=xyz!@#"), Err(InvoiceError::InvalidAddress));
    }

    #[test]
    fn test_invalid_amount() {
        assert_eq!(Invoice::from_uri("coins://pay?addr=abc123&amount=notanumber"), Err(InvoiceError::InvalidAmount));
    }

    #[test]
    fn test_invalid_token_id() {
        assert_eq!(Invoice::from_uri("coins://pay?addr=abc123&token=notanumber"), Err(InvoiceError::InvalidTokenId));
    }

    #[test]
    fn test_invalid_expires() {
        assert_eq!(Invoice::from_uri("coins://pay?addr=abc123&expires=notanumber"), Err(InvoiceError::InvalidExpires));
    }

    #[test]
    fn test_memo_url_encoding() {
        let inv = Invoice {
            addr: "abc123".to_string(),
            amount: None,
            token_id: None,
            memo: Some("Hello World!".to_string()),
            expires: None,
        };
        let uri = inv.to_uri();
        assert!(uri.contains("memo=Hello%20World%21"));

        let parsed = Invoice::from_uri(&uri).unwrap();
        assert_eq!(parsed.memo, Some("Hello World!".to_string()));
    }

    #[test]
    fn test_expiration_check() {
        // Past timestamp - should be expired
        let inv = Invoice {
            addr: "abc123".to_string(),
            amount: None,
            token_id: None,
            memo: None,
            expires: Some(1000000000), // Year 2001
        };
        assert!(inv.is_expired());

        // Far future timestamp - should not be expired
        let inv2 = Invoice {
            addr: "abc123".to_string(),
            amount: None,
            token_id: None,
            memo: None,
            expires: Some(9999999999),
        };
        assert!(!inv2.is_expired());

        // No expiration - should never be expired
        let inv3 = Invoice::new("abc123".to_string());
        assert!(!inv3.is_expired());
    }

    #[test]
    fn test_unknown_params_ignored() {
        let inv = Invoice::from_uri("coins://pay?addr=abc123&unknown=value&amount=50").unwrap();
        assert_eq!(inv.addr, "abc123");
        assert_eq!(inv.amount, Some(50));
    }

    #[test]
    fn test_malformed_uri() {
        assert_eq!(Invoice::from_uri("coins://pay?badparam"), Err(InvoiceError::MalformedUri));
    }
}
