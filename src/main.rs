// Copyright 2025 Maya Kaczorowski
//
// Licensed under the Apache License, Version 2.0 (the "License");
// you may not use this file except in compliance with the License.
// You may obtain a copy of the License at
//
//     http://www.apache.org/licenses/LICENSE-2.0
//
// Unless required by applicable law or agreed to in writing, software
// distributed under the License is distributed on an "AS IS" BASIS,
// WITHOUT WARRANTIES OR CONDITIONS OF ANY KIND, either express or implied.
// See the License for the specific language governing permissions and
// limitations under the License.
use clap::Parser;
use csv::Writer;
use std::error::Error;
use std::fs::File;
use std::io::{self, BufRead};
use std::path::Path;
use thirtyfour::prelude::*;

static URL_BASE: &str = "https://marketplace.fedramp.gov/products/";

#[derive(Parser, Debug)]
#[command(author, version, about = "FedRAMP Marketplace Scraper")]
struct Args {
    #[arg(
        short,
        long,
        default_value_t = 4444,
        help = "Port number for the WebDriver connection (default: 4444)"
    )]
    port: u16,

    #[arg(
        short,
        long,
        help = "Path to input file containing FedRAMP product IDs (one ID per line)",
        required = true
    )]
    input: String,

    #[arg(
        short,
        long,
        help = "Path where the output CSV file will be saved",
        required = true
    )]
    output: String,
}

#[derive(Debug)]
struct AuthorizationDetails {
    id: String,
    fedramp_ready: Option<String>,
    authorizing_entity_review: Option<String>,
    pmo_review: Option<String>,
    fedramp_authorized: Option<String>,
    annual_assessment: Option<String>,
    independent_assessor: Option<String>,
}

fn read_lines<P: AsRef<Path>>(filename: P) -> io::Result<io::Lines<io::BufReader<File>>> {
    Ok(io::BufReader::new(File::open(filename)?).lines())
}

async fn get_authorization_details(
    driver: &WebDriver,
    id: &str,
) -> Result<AuthorizationDetails, Box<dyn Error + Send + Sync>> {
    let auth_section = driver
        .query(By::XPath(
            "//h3[contains(text(),'Authorization Details')]/parent::div",
        ))
        .first()
        .await?;

    let paragraphs = auth_section.find_all(By::Tag("p")).await?;
    if paragraphs.is_empty() {
        return Err("No paragraphs found".into());
    }

    let mut details = AuthorizationDetails {
        id: id.to_string(),
        fedramp_ready: None,
        authorizing_entity_review: None,
        pmo_review: None,
        fedramp_authorized: None,
        annual_assessment: None,
        independent_assessor: None,
    };

    let extract_value = |text: &str, prefix: &str| -> Option<String> {
        text.split(prefix)
            .nth(1)
            .map(str::trim)
            .filter(|s| !s.is_empty())
            .map(String::from)
    };

    for p in paragraphs {
        let text = match p.text().await {
            Ok(t) => t,
            Err(_) => continue,
        };

        if text.contains("Independent Assessor:") {
            details.independent_assessor = extract_value(&text, "Independent Assessor:");
        } else if text.contains("FedRAMP Ready:") {
            details.fedramp_ready = extract_value(&text, "FedRAMP Ready:");
        } else if text.contains("Authorizing Entity Review:") {
            details.authorizing_entity_review = extract_value(&text, "Authorizing Entity Review:");
        } else if text.contains("PMO Review:") {
            details.pmo_review = extract_value(&text, "PMO Review:");
        } else if text.contains("FedRAMP Authorized:") {
            details.fedramp_authorized = extract_value(&text, "FedRAMP Authorized:");
        } else if text.contains("Annual Assessment:") {
            details.annual_assessment = extract_value(&text, "Annual Assessment:");
        }
    }

    Ok(details)
}

#[tokio::main]
async fn main() -> Result<(), Box<dyn Error + Send + Sync>> {
    let args = Args::parse();

    let caps = DesiredCapabilities::chrome();
    let driver = WebDriver::new(&format!("http://localhost:{}", args.port), caps).await?;

    let ids: Vec<String> = read_lines(&args.input)?.map_while(Result::ok).collect();
    eprintln!("Found {} IDs to process", ids.len());

    let mut wtr = Writer::from_writer(File::create(&args.output)?);
    wtr.write_record([
        "ID",
        "FedRAMP Ready",
        "Authorizing Entity Review",
        "PMO Review",
        "FedRAMP Authorized",
        "Annual Assessment",
        "Independent Assessor",
    ])?;

    for (i, id) in ids.iter().enumerate() {
        eprintln!("[{}/{}] Processing ID: {}", i + 1, ids.len(), id);

        if let Err(e) = driver.goto(format!("{}{}", URL_BASE, id)).await {
            eprintln!("Error navigating to ID {}: {}", id, e);
            wtr.write_record([id, "Error - Navigation failed", "", "", "", "", ""])?;
            wtr.flush()?;
            continue;
        }

        driver.refresh().await?;
        match get_authorization_details(&driver, id).await {
            Ok(details) => {
                wtr.write_record([
                    &details.id,
                    &details.fedramp_ready.unwrap_or_default(),
                    &details.authorizing_entity_review.unwrap_or_default(),
                    &details.pmo_review.unwrap_or_default(),
                    &details.fedramp_authorized.unwrap_or_default(),
                    &details.annual_assessment.unwrap_or_default(),
                    &details.independent_assessor.unwrap_or_default(),
                ])?;
                eprintln!("Successfully scraped data for ID: {}", id);
            }
            Err(e) => {
                eprintln!("Error processing ID {}: {}", id, e);
                wtr.write_record([id, &format!("Error: {}", e), "", "", "", "", ""])?;
            }
        }
        wtr.flush()?;
    }

    driver.close_window().await?;
    eprintln!("Scraping completed. Results saved to {}", args.output);
    Ok(())
}
