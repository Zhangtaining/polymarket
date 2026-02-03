use anyhow::Result;
use scraper::{Html, Selector};
use std::str::FromStr;

const POLYMARKET_BASE_URL: &str = "https://polymarket.com/event";

#[derive(Debug, Clone)]
pub struct ScrapedPriceData {
    pub open_price: f64,
    pub close_price: Option<f64>,
    pub start_time: Option<String>,
    pub end_time: Option<String>,
}

/// Fetch the "Price to Beat" (open price at window start) from the Polymarket event page.
/// Polymarket embeds the open price in the page as JSON: "openPrice":77572.06425014541
/// (this is the Chainlink BTC/USD price at the start of the 15-min window, e.g. 77,572.06).
/// We try embedded JSON first (reliable), then fall back to the "price to beat" div if present.
pub async fn fetch_price_to_beat(market_slug: &str) -> Result<Option<ScrapedPriceData>> {
    let url = format!("{}/{}", POLYMARKET_BASE_URL, market_slug);

    let client = reqwest::Client::builder()
        .user_agent("Mozilla/5.0 (Macintosh; Intel Mac OS X 10_15_7) AppleWebKit/537.36 (KHTML, like Gecko) Chrome/120.0.0.0 Safari/537.36")
        .build()?;

    let response = client.get(&url).send().await?;

    if !response.status().is_success() {
        tracing::warn!("Failed to fetch page {}: {}", url, response.status());
        return Ok(None);
    }

    let html = response.text().await?;

    // Primary: extract openPrice from embedded JSON (same number shown as "price to beat" on the page)
    if let Some(open_price) = extract_open_price_from_embedded_json(&html) {
        tracing::debug!("Found openPrice {:.2} from embedded JSON (price to beat)", open_price);
        return Ok(Some(ScrapedPriceData {
            open_price,
            close_price: None,
            start_time: None,
            end_time: None,
        }));
    }

    // Fallback: parse the "price to beat" div (may be missing in initial HTML for client-rendered content)
    if let Some(open_price) = extract_price_to_beat_from_html(&html) {
        return Ok(Some(ScrapedPriceData {
            open_price,
            close_price: None,
            start_time: None,
            end_time: None,
        }));
    }

    Ok(None)
}

/// Extract the current window's open price from embedded JSON in the page.
/// The page contains React Query / cache data with "openPrice":<number> for each 15m window.
/// The last occurrence is the current window's open (the "price to beat" = 77,572.06 etc).
fn extract_open_price_from_embedded_json(html: &str) -> Option<f64> {
    let pattern = "\"openPrice\":";
    let mut last_open_price: Option<f64> = None;
    let mut search_start = 0;

    while let Some(pos) = html[search_start..].find(pattern) {
        let value_start = search_start + pos + pattern.len();
        let value_end = html[value_start..]
            .find(|c: char| c == ',' || c == '}' || c == ' ')
            .map(|i| value_start + i)
            .unwrap_or(html.len());
        let value_str = html[value_start..value_end].trim();
        if let Ok(p) = value_str.parse::<f64>() {
            // Sanity: BTC price should be in a reasonable range (e.g. 10k–200k)
            if p >= 10_000.0 && p <= 500_000.0 {
                last_open_price = Some(p);
            }
        }
        search_start = value_end;
    }

    last_open_price
}

/// Find the div that contains the "price to beat" label and extract the price value.
/// The section looks like: <div class="flex items-center gap-1 justify-between">
///   <span ...>price to beat</span>
///   <span or other element with the price number>
/// </div>
fn extract_price_to_beat_from_html(html: &str) -> Option<f64> {
    let document = Html::parse_document(html);

    // Select divs that have "justify-between" in class (Tailwind layout for label + value)
    let div_selector = Selector::parse("div[class*=\"justify-between\"]").ok()?;

    for div in document.select(&div_selector) {
        let text = div.text().collect::<String>();
        // Check if this div contains the "price to beat" label
        if !text.to_lowercase().contains("price to beat") {
            continue;
        }

        // The price is typically the sibling of the label span (second child in justify-between).
        let price = extract_price_from_element_text(&div);
        if let Some(p) = price {
            tracing::debug!("Found price to beat {} in HTML section", p);
            return Some(p);
        }
        // Fallback: price might be in the same text block (e.g. "price to beat $12345.67")
        let full_text = div.text().collect::<String>();
        if let Some(p) = extract_price_from_text(&full_text) {
            tracing::debug!("Found price to beat {} from div text", p);
            return Some(p);
        }
    }

    None
}

/// Extract a price from a string that may contain "price to beat" and a number like $12,345.67.
fn extract_price_from_text(s: &str) -> Option<f64> {
    // Find the last number that looks like a price (optional $, digits, optional , and .)
    let mut last_price: Option<f64> = None;
    let mut i = 0;
    let bytes = s.as_bytes();
    while i < bytes.len() {
        if bytes[i] == b'$' || bytes[i].is_ascii_digit() {
            let start = i;
            if bytes[i] == b'$' {
                i += 1;
            }
            while i < bytes.len() && (bytes[i].is_ascii_digit() || bytes[i] == b',' || bytes[i] == b'.') {
                i += 1;
            }
            let segment = std::str::from_utf8(&bytes[start..i]).ok()?;
            if let Some(p) = parse_price_string(segment) {
                last_price = Some(p);
            }
        } else {
            i += 1;
        }
    }
    last_price
}

/// Extract a price (f64) from an element: find text that looks like a number ($12,345.67 or 12345.67).
fn extract_price_from_element_text(element: &scraper::ElementRef) -> Option<f64> {
    use scraper::ElementRef;

    // Collect text from direct children; in "justify-between" the second child usually has the value
    let mut last_text_with_number: Option<String> = None;
    for child in element.children() {
        let node = child.value();
        if let scraper::node::Node::Element(_) = node {
            if let Some(child_el) = ElementRef::wrap(child) {
                let s = child_el.text().collect::<String>().trim().to_string();
                if !s.is_empty() && s != "price to beat" && s != "PRICE TO BEAT" {
                    if parse_price_string(&s).is_some() {
                        last_text_with_number = Some(s);
                    }
                }
            }
        }
    }

    last_text_with_number.and_then(|s| parse_price_string(&s))
}

/// Parse a string that may look like "$12,345.67" or "12345.67" into f64.
fn parse_price_string(s: &str) -> Option<f64> {
    let cleaned: String = s
        .trim()
        .replace(',', "")
        .chars()
        .filter(|c| c.is_ascii_digit() || *c == '.' || *c == '-')
        .collect();
    if cleaned.is_empty() {
        return None;
    }
    f64::from_str(&cleaned).ok()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_extract_open_price_from_embedded_json() {
        // Simulates embedded JSON from Polymarket event page (price to beat = 77,572.06)
        let html = r#"past windows..."openPrice":77461.786,"closePrice":77180.0},{"startTime":"2026-02-01T17:00:00.000Z","endTime":"2026-02-01T17:15:00.000Z","openPrice":77423.78,"closePrice":77572.06425014541},{"startTime":"2026-02-01T17:15:00.000Z","endTime":"2026-02-01T17:30:00.000Z","openPrice":77572.06425014541,"closePrice":77490.31},"dataUpdateCount":1"#;
        let open = extract_open_price_from_embedded_json(html);
        assert!(open.is_some());
        let p = open.unwrap();
        assert!((p - 77572.06425014541).abs() < 1e-6, "expected ~77572.06, got {}", p);
    }

    #[tokio::test]
    #[ignore] // requires network
    async fn test_fetch_price_to_beat() {
        let result = fetch_price_to_beat("btc-updown-15m-1769959800").await;
        match result {
            Ok(Some(data)) => {
                println!("Open Price: ${:.2}", data.open_price);
                if let Some(close) = data.close_price {
                    println!("Close Price: ${:.2}", close);
                }
            }
            Ok(None) => println!("No price data found"),
            Err(e) => println!("Error: {:?}", e),
        }
    }
}
