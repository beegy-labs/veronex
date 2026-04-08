use async_trait::async_trait;
use chrono::{DateTime, Datelike, Offset, Timelike, Utc};
use chrono_tz::Tz;
use serde_json::{Value, json};
use super::Tool;

pub struct DateTimeTool;

impl DateTimeTool {
    pub fn new() -> Self { Self }
}

#[async_trait]
impl Tool for DateTimeTool {
    fn spec(&self) -> Value {
        json!({
            "name": "get_datetime",
            "description": "Returns the current date and time. Optionally converts to a specific IANA timezone (e.g. 'Asia/Seoul', 'America/New_York', 'Europe/London'). Use this whenever the user asks about the current time, date, day of week, or needs time zone conversion.",
            "inputSchema": {
                "type": "object",
                "properties": {
                    "timezone": {
                        "type": "string",
                        "description": "IANA timezone name (e.g. 'Asia/Seoul', 'UTC', 'America/New_York'). Defaults to UTC if not provided."
                    }
                },
                "required": []
            },
            "annotations": {
                "readOnlyHint": true,
                "idempotentHint": false,
                "destructiveHint": false,
                "openWorldHint": false
            }
        })
    }

    async fn call(&self, args: &Value) -> Result<Value, String> {
        let tz_str = args["timezone"].as_str().unwrap_or("UTC");

        let now_utc: DateTime<Utc> = Utc::now();

        let (iso, date, time, utc_offset, timezone, day_of_week) = if tz_str == "UTC" {
            let formatted = now_utc.format("%Y-%m-%dT%H:%M:%SZ").to_string();
            let date = now_utc.format("%Y-%m-%d").to_string();
            let time = now_utc.format("%H:%M:%S").to_string();
            let dow = weekday_name(now_utc.weekday());
            (formatted, date, time, "+00:00".to_string(), "UTC".to_string(), dow)
        } else {
            let tz: Tz = tz_str.parse().map_err(|_| format!("Unknown timezone: '{tz_str}'. Use IANA format like 'Asia/Seoul'."))?;
            let local = now_utc.with_timezone(&tz);
            let iso = local.format("%Y-%m-%dT%H:%M:%S%:z").to_string();
            let date = local.format("%Y-%m-%d").to_string();
            let time = local.format("%H:%M:%S").to_string();
            let offset_secs = local.offset().fix().local_minus_utc();
            let offset_h = offset_secs / 3600;
            let offset_m = (offset_secs.abs() % 3600) / 60;
            let utc_offset = format!("{:+03}:{:02}", offset_h, offset_m);
            let dow = weekday_name(local.weekday());
            (iso, date, time, utc_offset, tz_str.to_string(), dow)
        };

        Ok(json!({
            "iso":        iso,
            "unix_epoch": now_utc.timestamp(),
            "timezone":   timezone,
            "utc_offset": utc_offset,
            "date":       date,
            "time":       time,
            "day_of_week": day_of_week,
            "year":  now_utc.year(),
            "month": now_utc.month(),
            "day":   now_utc.day(),
            "hour":  now_utc.hour(),
            "minute": now_utc.minute()
        }))
    }
}

fn weekday_name(wd: chrono::Weekday) -> &'static str {
    match wd {
        chrono::Weekday::Mon => "Monday",
        chrono::Weekday::Tue => "Tuesday",
        chrono::Weekday::Wed => "Wednesday",
        chrono::Weekday::Thu => "Thursday",
        chrono::Weekday::Fri => "Friday",
        chrono::Weekday::Sat => "Saturday",
        chrono::Weekday::Sun => "Sunday",
    }
}

