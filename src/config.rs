use embassy_time::Duration;

use crate::Entity;

pub const SSID: &str = "<Wi-Fi SSID>";
pub const PASS: &str = "<Wi-Fi Password>";
pub const HA_API_URL: &str = "https://yourhomeassistantinstance.com/api/states/";
pub const TOKEN: &str = "Bearer <Home Assistant Token>";

pub const NUM_ENTITIES: usize = 5;
pub const ENTITIES: [Entity; NUM_ENTITIES] = [
    Entity {
        display_name: "I Temp",
        display_unit: b"\xB0C",
        entity_name: "sensor.temperature",
        position: 0,
    },
    Entity {
        display_name: "I Humid",
        display_unit: b"%",
        entity_name: "sensor.humidity",
        position: 16,
    },
    Entity {
        display_name: "VOC",
        display_unit: b"",
        entity_name: "sensor.voc",
        position: 32,
    },
    Entity {
        display_name: "O Temp",
        display_unit: b"\xB0C",
        entity_name: "sensor.outside_temperature",
        position: 40,
    },
    Entity {
        display_name: "PM <2.5",
        display_unit: b"ug/m\xB3",
        entity_name: "sensor.pm_2_5um_weight_concentration",
        position: 56,
    },
];

pub const REFRESH_EVERY: Duration = Duration::from_millis(5000);

pub const DEFAULT_LUMINANCE: u8 = 100;
pub const LUMINANCE_ENTITY: Option<&str> = Some("input_number.luminance");
