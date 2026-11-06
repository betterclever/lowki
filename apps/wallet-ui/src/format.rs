//! Turning wallet data into plain sentences: addresses, dates, delegation
//! policies and EIP-712 payloads.

use serde_json::Value;

use crate::backend::{Amount, DelegationPolicy, PolicyRule, Token};

/// "0x7a25…488d".
pub fn short_addr(a: &str) -> String {
    if a.starts_with("0x") && a.len() > 14 {
        format!("{}…{}", &a[..6], &a[a.len() - 4..])
    } else {
        a.to_string()
    }
}

/// "0x3f2a9c…e81b" for hashes.
pub fn short_hash(h: &str) -> String {
    if h.len() > 16 {
        format!("{}…{}", &h[..8], &h[h.len() - 4..])
    } else {
        h.to_string()
    }
}

/// "2026-12-31" -> "31 Dec 2026".
pub fn human_date(iso: &str) -> String {
    const M: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];
    let parts: Vec<&str> = iso.split('-').collect();
    if let [y, m, d] = parts.as_slice()
        && let (Ok(m), Ok(d)) = (m.parse::<usize>(), d.parse::<u32>())
        && (1..=12).contains(&m)
    {
        return format!("{d} {} {y}", M[m - 1]);
    }
    iso.to_string()
}

/// Unix seconds -> "31 Dec 2026" (UTC), without any date library.
pub fn unix_date(secs: i64) -> String {
    // Howard Hinnant's civil_from_days.
    let z = secs.div_euclid(86_400) + 719_468;
    let era = z.div_euclid(146_097);
    let doe = z - era * 146_097;
    let yoe = (doe - doe / 1460 + doe / 36_524 - doe / 146_096) / 365;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    let y = yoe + era * 400 + i64::from(m <= 2);
    human_date(&format!("{y}-{m:02}-{d:02}"))
}

pub fn chain_name(c: &str) -> String {
    match c.to_ascii_lowercase().as_str() {
        "monad" | "10143" | "143" => "Monad".into(),
        "ethereum" | "mainnet" | "1" => "Ethereum".into(),
        "base" | "8453" => "Base".into(),
        other => {
            let mut s = other.to_string();
            if let Some(f) = s.get_mut(0..1) {
                f.make_ascii_uppercase();
            }
            s
        }
    }
}

fn cap_phrase(max: &str, token: &str) -> String {
    format!("up to {max} {token} per day")
}

fn fn_name(sig: &str) -> String {
    sig.split('(').next().unwrap_or(sig).to_string()
}

/// One sentence per allowed action, e.g.
/// "Swap on router 0x7a25…488d, up to 50 USDC per day, on Monad, until 31 Dec 2026".
pub fn policy_can(p: &DelegationPolicy) -> Vec<String> {
    let tail = format!("on {}, until {}", chain_name(&p.chain), human_date(&p.expires));
    p.rules
        .iter()
        .map(|r| match r {
            PolicyRule::Swap { router, router_label, token, max_per_day } => {
                let who = match router_label {
                    Some(l) => format!("{l} router {}", short_addr(router)),
                    None => format!("router {}", short_addr(router)),
                };
                format!("Swap on {who}, {}, {tail}", cap_phrase(max_per_day, token))
            }
            PolicyRule::Call { contract, contract_label, function, max_per_day } => {
                let on = match contract_label {
                    Some(l) => format!("{l} ({})", short_addr(contract)),
                    None => short_addr(contract),
                };
                let times = max_per_day.map(|n| format!(", up to {n} times per day")).unwrap_or_default();
                format!("Call {} on {on}{times}, {tail}", fn_name(function))
            }
            PolicyRule::Transfer { to, token, max_per_day } => {
                format!("Send {} to {} only, {tail}", cap_phrase(max_per_day, token), short_addr(to))
            }
        })
        .collect()
}

/// The things the signer can never do, said plainly.
pub fn policy_cant(p: &DelegationPolicy) -> Vec<String> {
    let mut v = Vec::new();
    if !p.rules.iter().any(|r| matches!(r, PolicyRule::Transfer { .. })) {
        v.push("Send to other addresses".to_string());
    } else {
        v.push("Send to any address not listed above".to_string());
    }
    v.push("See your other apps or who you are".into());
    v.push("Touch your private balance".into());
    v.push("Go past these limits or change them".into());
    v.push(format!("Do anything after {}", human_date(&p.expires)));
    v
}

/// A decoded EIP-712 row.
#[derive(Clone, Debug, PartialEq)]
pub struct Field {
    pub label: String,
    pub value: String,
    /// Full value for copy/tooltips (addresses).
    pub full: Option<String>,
    pub depth: u8,
}

/// "maxPrice" -> "Max price".
pub fn humanize(key: &str) -> String {
    let mut out = String::new();
    for (i, c) in key.chars().enumerate() {
        if c == '_' {
            out.push(' ');
        } else if c.is_uppercase() && i > 0 {
            out.push(' ');
            out.extend(c.to_lowercase());
        } else if i == 0 {
            out.extend(c.to_uppercase());
        } else {
            out.push(c);
        }
    }
    out
}

fn as_u128(v: &Value) -> Option<u128> {
    match v {
        Value::Number(n) => n.as_u64().map(u128::from),
        Value::String(s) => s.parse().ok(),
        _ => None,
    }
}

pub struct TypedView {
    pub primary: String,
    pub domain: Vec<Field>,
    pub message: Vec<Field>,
}

/// Decodes an eth_signTypedData_v4 payload into labelled rows, following the
/// declared field order. `token_at` resolves token contracts so amounts next
/// to them can be shown in whole units.
pub fn decode_typed(v: &Value, token_at: &dyn Fn(&str) -> Option<Token>) -> Option<TypedView> {
    let types = v.get("types")?.as_object()?;
    let primary = v.get("primaryType")?.as_str()?.to_string();
    let msg = v.get("message")?;
    let mut domain = Vec::new();
    if let Some(d) = v.get("domain").and_then(Value::as_object) {
        for (k, label) in [("name", "App"), ("version", "Version"), ("chainId", "Network"), ("verifyingContract", "Contract")] {
            if let Some(x) = d.get(k) {
                let (value, full) = match k {
                    "chainId" => (chain_name(x.to_string().trim_matches('"')), None),
                    "verifyingContract" => {
                        let s = x.as_str().unwrap_or_default().to_string();
                        (short_addr(&s), Some(s))
                    }
                    _ => (x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()), None),
                };
                domain.push(Field { label: label.into(), value, full, depth: 0 });
            }
        }
    }
    let mut message = Vec::new();
    walk(types, &primary, msg, 0, token_at, &mut message);
    Some(TypedView { primary, domain, message })
}

fn walk(
    types: &serde_json::Map<String, Value>,
    ty: &str,
    val: &Value,
    depth: u8,
    token_at: &dyn Fn(&str) -> Option<Token>,
    out: &mut Vec<Field>,
) {
    let Some(fields) = types.get(ty).and_then(Value::as_array) else { return };
    // A token address among the siblings gives units to amount-like fields.
    let token = fields.iter().find_map(|f| {
        let name = f.get("name")?.as_str()?;
        (f.get("type")?.as_str()? == "address").then(|| val.get(name)?.as_str().and_then(token_at)).flatten()
    });
    for f in fields {
        let (Some(name), Some(fty)) = (f.get("name").and_then(Value::as_str), f.get("type").and_then(Value::as_str)) else {
            continue;
        };
        let x = val.get(name).unwrap_or(&Value::Null);
        let label = humanize(name);
        if types.contains_key(fty) {
            out.push(Field { label, value: String::new(), full: None, depth });
            walk(types, fty, x, depth + 1, token_at, out);
            continue;
        }
        let (value, full) = if fty == "address" {
            let s = x.as_str().unwrap_or_default().to_string();
            match token_at(&s) {
                Some(t) => (format!("{} ({})", t.symbol(), short_addr(&s)), Some(s)),
                None => (short_addr(&s), Some(s)),
            }
        } else if fty.starts_with("uint") || fty.starts_with("int") {
            let lname = name.to_ascii_lowercase();
            let amountish = ["price", "amount", "value", "fee"].iter().any(|k| lname.contains(k));
            match (as_u128(x), token, amountish) {
                (Some(n), Some(t), true) => (Amount::new(t, n).fmt(), None),
                (Some(n), _, _) if lname.contains("deadline") || lname.contains("expir") => (unix_date(n as i64), None),
                (Some(n), _, _) => (n.to_string(), None),
                _ => (x.to_string(), None),
            }
        } else if fty == "bool" {
            (if x.as_bool().unwrap_or(false) { "Yes" } else { "No" }.to_string(), None)
        } else if fty.starts_with("bytes") {
            let s = x.as_str().unwrap_or_default().to_string();
            (short_hash(&s), Some(s))
        } else {
            (x.as_str().map(str::to_string).unwrap_or_else(|| x.to_string()), None)
        };
        out.push(Field { label, value, full, depth });
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn sentences() {
        let p = DelegationPolicy {
            chain: "monad".into(),
            expires: "2026-12-31".into(),
            rules: vec![PolicyRule::Swap {
                router: "0x7a250d5630b4cf539739df2c5dacb4c659f2488d".into(),
                router_label: None,
                token: "USDC".into(),
                max_per_day: "50".into(),
            }],
        };
        assert_eq!(policy_can(&p)[0], "Swap on router 0x7a25…488d, up to 50 USDC per day, on Monad, until 31 Dec 2026");
        assert!(policy_cant(&p).contains(&"Touch your private balance".to_string()));
    }

    #[test]
    fn amounts() {
        let a = Amount::parse(Token::Usdc, "1234.5").unwrap();
        assert_eq!(a.fmt(), "1,234.50 USDC");
        assert!(Amount::parse(Token::Usdc, "1.1234567").is_none());
        assert_eq!(humanize("maxPrice"), "Max price");
        assert_eq!(unix_date(1_798_675_200), "31 Dec 2026");
    }
}
