use std::path::{Path, PathBuf};

use anyhow::Result;

pub struct WeatherFixtures {
    pub adaptive_card: PathBuf,
    pub templates: PathBuf,
    pub weatherapi: Option<PathBuf>,
    pub mock_weather: PathBuf,
}

pub fn load_weather_fixtures(root: &Path) -> Result<WeatherFixtures> {
    let base = root.join("tests/fixtures/real_components");
    let adaptive_card = base.join("adaptive_card/component.wasm");
    let templates = base.join("templates/component.wasm");
    let weatherapi = base.join("weatherapi/weatherapi.wasm");
    let mock_weather = root.join("tests/fixtures/external_mocks/weatherapi_london_3days.json");

    if !adaptive_card.exists() || !templates.exists() {
        anyhow::bail!("adaptive_card/templates fixtures missing; run refresh_real_components.sh");
    }

    let weatherapi_path = if weatherapi.exists() {
        Some(weatherapi)
    } else {
        None
    };

    if !mock_weather.exists() {
        anyhow::bail!("mock weather fixture missing at {}", mock_weather.display());
    }

    Ok(WeatherFixtures {
        adaptive_card,
        templates,
        weatherapi: weatherapi_path,
        mock_weather,
    })
}
