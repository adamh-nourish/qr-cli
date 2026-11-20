//! Locations as `geo:` URIs (RFC 5870).

use std::fmt;

use super::PayloadError;

/// A point on the WGS 84 globe: `geo:<lat>,<lon>`.
///
/// Coordinates are written in the shortest decimal form that reads back as
/// the same number, never in exponent notation, and `-0` is written as
/// `0`.
///
/// ```
/// use qr_core::payload::Geo;
/// assert_eq!(Geo::new(51.5007, -0.1246)?.to_string(), "geo:51.5007,-0.1246");
/// assert!(Geo::new(91.0, 0.0).is_err());
/// # Ok::<(), qr_core::payload::PayloadError>(())
/// ```
#[derive(Debug, Clone, Copy, PartialEq)]
pub struct Geo {
    latitude: f64,
    longitude: f64,
}

// Both coordinates are finite (checked in `new`), so equality is total.
impl Eq for Geo {}

impl Geo {
    /// Validates the coordinates, in decimal degrees.
    ///
    /// # Errors
    ///
    /// [`PayloadError::Invalid`] when the latitude is outside -90 to 90,
    /// the longitude outside -180 to 180, or either is not finite.
    pub fn new(latitude: f64, longitude: f64) -> Result<Self, PayloadError> {
        check("latitude", latitude, 90.0, "must be between -90 and 90")?;
        check(
            "longitude",
            longitude,
            180.0,
            "must be between -180 and 180",
        )?;
        Ok(Self {
            // Adding 0.0 turns -0.0 into 0.0.
            latitude: latitude + 0.0,
            longitude: longitude + 0.0,
        })
    }

    /// The latitude in decimal degrees.
    pub const fn latitude(&self) -> f64 {
        self.latitude
    }

    /// The longitude in decimal degrees.
    pub const fn longitude(&self) -> f64 {
        self.longitude
    }
}

fn check(
    field: &'static str,
    value: f64,
    max: f64,
    reason: &'static str,
) -> Result<(), PayloadError> {
    if value.is_finite() && (-max..=max).contains(&value) {
        Ok(())
    } else {
        Err(PayloadError::invalid(field, &value.to_string(), reason))
    }
}

impl fmt::Display for Geo {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        // `f64`'s Display is the shortest round-trip form, without exponent.
        write!(f, "geo:{},{}", self.latitude, self.longitude)
    }
}

#[cfg(test)]
mod tests {
    use proptest::prelude::*;

    use super::*;

    #[test]
    fn formats_coordinates() {
        for (lat, lon, expected) in [
            (0.0, 0.0, "geo:0,0"),
            (-0.0, -0.0, "geo:0,0"),
            (90.0, 180.0, "geo:90,180"),
            (-90.0, -180.0, "geo:-90,-180"),
            (1e-7, 0.5, "geo:0.0000001,0.5"),
            (37.786_971, -122.399_677, "geo:37.786971,-122.399677"),
        ] {
            let geo = Geo::new(lat, lon).unwrap();
            assert_eq!(geo.to_string(), expected);
        }
        let geo = Geo::new(1.5, 2.5).unwrap();
        assert_eq!((geo.latitude(), geo.longitude()), (1.5, 2.5));
    }

    #[test]
    fn rejects_out_of_range() {
        for (lat, lon, field) in [
            (90.000_1, 0.0, "latitude"),
            (-91.0, 0.0, "latitude"),
            (f64::NAN, 0.0, "latitude"),
            (f64::INFINITY, 0.0, "latitude"),
            (0.0, 180.5, "longitude"),
            (0.0, f64::NEG_INFINITY, "longitude"),
        ] {
            let err = Geo::new(lat, lon).unwrap_err();
            assert!(
                matches!(err, PayloadError::Invalid { field: f, .. } if f == field),
                "{err}"
            );
        }
        assert_eq!(
            Geo::new(91.0, 0.0).unwrap_err().to_string(),
            "invalid latitude '91': must be between -90 and 90"
        );
    }

    proptest! {
        #[test]
        fn coordinates_round_trip_through_the_text(
            lat in -90.0f64..=90.0,
            lon in -180.0f64..=180.0,
        ) {
            let text = Geo::new(lat, lon).unwrap().to_string();
            let rest = text.strip_prefix("geo:").unwrap();
            prop_assert!(!rest.contains(['e', 'E']));
            let (a, b) = rest.split_once(',').unwrap();
            prop_assert_eq!(a.parse::<f64>().unwrap().to_bits(), (lat + 0.0).to_bits());
            prop_assert_eq!(b.parse::<f64>().unwrap().to_bits(), (lon + 0.0).to_bits());
        }
    }
}
