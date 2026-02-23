//! Google Sheets integration: fetch market config and hyperparameters via CSV export.

use anyhow::Result;
use csv::Reader;
use std::collections::HashMap;

use crate::types::{MarketRow, ParamsMap};

const SHEET_GIDS: &[(&str, u64)] = &[
    ("Selected Markets", 3),
    ("All Markets", 1),
    ("Hyperparameters", 4),
];

/// Fetch CSV for a worksheet by sheet title (using public export URL).
fn fetch_sheet_csv(sheet_id: &str, sheet_title: &str, gid: u64) -> Result<String> {
    let url = format!(
        "https://docs.google.com/spreadsheets/d/{}/export?format=csv&gid={}",
        sheet_id, gid
    );
    let resp = reqwest::blocking::get(&url)?;
    let text = resp.text()?;
    Ok(text)
}

fn parse_selected_markets(csv: &str) -> Result<Vec<HashMap<String, String>>> {
    let mut rdr = Reader::from_reader(csv.as_bytes());
    let headers: Vec<String> = rdr
        .headers()?
        .iter()
        .map(|h| h.to_string())
        .collect();
    let mut rows = Vec::new();
    for result in rdr.records() {
        let record = result?;
        let mut row = HashMap::new();
        for (i, field) in record.iter().enumerate() {
            if i < headers.len() {
                row.insert(headers[i].clone(), field.to_string());
            }
        }
        rows.push(row);
    }
    Ok(rows)
}

fn parse_hyperparameters(csv: &str) -> Result<ParamsMap> {
    let mut rdr = Reader::from_reader(csv.as_bytes());
    let headers = rdr.headers()?.clone();
    let mut params = ParamsMap::new();
    let mut current_type: Option<String> = None;

    for result in rdr.records() {
        let record = result?;
        let type_col = headers.iter().position(|h| h == "type").unwrap_or(0);
        let param_col = headers.iter().position(|h| h == "param").unwrap_or(1);
        let value_col = headers.iter().position(|h| h == "value").unwrap_or(2);

        let type_val = record.get(type_col).unwrap_or("").trim();
        if !type_val.is_empty() {
            current_type = Some(type_val.to_string());
        }
        let Some(ref ct) = current_type else { continue };

        let param = record.get(param_col).unwrap_or("").to_string();
        let value_str = record.get(value_col).unwrap_or("").to_string();
        let value: serde_json::Value = if value_str
            .chars()
            .all(|c| c.is_ascii_digit() || c == '.' || c == '-')
        {
            value_str.parse::<f64>().map(serde_json::Value::from).unwrap_or(serde_json::Value::String(value_str))
        } else {
            serde_json::Value::String(value_str)
        };
        params
            .entry(ct.clone())
            .or_default()
            .insert(param, value);
    }
    Ok(params)
}

/// Extract sheet ID from Google Sheets URL.
pub fn extract_sheet_id(url: &str) -> Result<String> {
    let re = regex::Regex::new(r"/spreadsheets/d/([a-zA-Z0-9_-]+)")?;
    let cap = re
        .captures(url)
        .ok_or_else(|| anyhow::anyhow!("Invalid Google Sheets URL"))?;
    Ok(cap[1].to_string())
}

/// Get market dataframe and hyperparameters from Google Sheets (read-only CSV export).
pub fn get_sheet_df(spreadsheet_url: &str) -> Result<(Vec<MarketRow>, ParamsMap)> {
    let sheet_id = extract_sheet_id(spreadsheet_url)?;

    // Fetch Selected Markets and All Markets, then merge on question
    let sel_csv = fetch_sheet_csv(&sheet_id, "Selected Markets", 3)?;
    let all_csv = fetch_sheet_csv(&sheet_id, "All Markets", 1)?;
    let hyp_csv = fetch_sheet_csv(&sheet_id, "Hyperparameters", 4)?;

    let sel_rows = parse_selected_markets(&sel_csv)?;
    let all_rows = parse_selected_markets(&all_csv)?;
    let params = parse_hyperparameters(&hyp_csv)?;

    // Build All Markets by question
    let all_by_question: HashMap<String, HashMap<String, String>> = all_rows
        .into_iter()
        .filter(|r| r.get("question").map(|s| s != "").unwrap_or(false))
        .map(|r| {
            let q = r.get("question").cloned().unwrap_or_default();
            (q, r)
        })
        .collect();

    let mut result = Vec::new();
    for sel in sel_rows {
        let question = sel.get("question").cloned().unwrap_or_default();
        if question.is_empty() {
            continue;
        }
        let Some(all) = all_by_question.get(&question) else { continue };

        let condition_id = all.get("condition_id").cloned().unwrap_or_else(|| sel.get("condition_id").cloned().unwrap_or_default());
        let token1 = all.get("token1").cloned().unwrap_or_default();
        let token2 = all.get("token2").cloned().unwrap_or_default();
        let answer1 = all.get("answer1").cloned().unwrap_or_default();
        let answer2 = all.get("answer2").cloned().unwrap_or_default();
        let tick_size: f64 = all.get("tick_size").and_then(|s| s.parse().ok()).unwrap_or(0.01);
        let min_size: f64 = all.get("min_size").and_then(|s| s.parse().ok()).unwrap_or(1.0);
        let trade_size: f64 = sel.get("trade_size").and_then(|s| s.parse().ok()).unwrap_or(10.0);
        let max_size: Option<f64> = sel.get("max_size").and_then(|s| s.parse().ok());
        let max_spread: f64 = sel.get("max_spread").and_then(|s| s.parse().ok()).unwrap_or(5.0);
        let neg_risk = all.get("neg_risk").cloned().unwrap_or_else(|| "FALSE".to_string());
        let best_bid: f64 = sel.get("best_bid").and_then(|s| s.parse().ok()).unwrap_or(0.5);
        let best_ask: f64 = sel.get("best_ask").and_then(|s| s.parse().ok()).unwrap_or(0.5);
        let param_type = sel.get("param_type").cloned().unwrap_or_else(|| "default".to_string());
        let multiplier = sel.get("multiplier").cloned().unwrap_or_default();
        let three_hour: f64 = sel.get("3_hour").and_then(|s| s.parse().ok()).unwrap_or(0.0);

        result.push(MarketRow {
            condition_id,
            question,
            token1,
            token2,
            answer1,
            answer2,
            tick_size,
            min_size,
            trade_size,
            max_size,
            max_spread,
            neg_risk,
            best_bid,
            best_ask,
            param_type,
            multiplier,
            three_hour,
        });
    }

    Ok((result, params))
}
