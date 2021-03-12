use super::Result;
use std::ops::{Add, AddAssign, Mul};

// Kubernetes resouce structs
//
// These are used in manifests where T is a String
// but is generic herein because we can have a fully parsed version
// where all values are parsed as normalised f64s.
// This allows extra computation, and certain versions will have some extra traits
// implemented to be a bit more useful, as well as some to convert between them.

/// Kubernetes resource requests or limit
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct Resources<T> {
    /// CPU request string
    pub cpu: T,
    /// Memory request string
    pub memory: T,
    // TODO: ephemeral-storage + extended-resources
}

/// Kubernetes resources
///
/// This can be inlined straight into a container spec at the moment
#[derive(Serialize, Deserialize, Clone, Debug)]
#[cfg_attr(feature = "filesystem", serde(deny_unknown_fields))]
pub struct ResourceRequirements<T> {
    /// Resource requests for k8s
    pub requests: Resources<T>,
    /// Resource limits for k8s
    pub limits: Resources<T>,
}

impl ResourceRequirements<String> {
    /// Convert shorthand strings to raw number of cores and Bytes of memory
    pub fn normalised(&self) -> Result<ResourceRequirements<f64>> {
        let requests = Resources {
            memory: parse_memory(&self.requests.memory.to_string())?,
            cpu: parse_cpu(&self.requests.cpu.to_string())?,
        };
        let limits = Resources {
            memory: parse_memory(&self.limits.memory.to_string())?,
            cpu: parse_cpu(&self.limits.cpu.to_string())?,
        };
        Ok(ResourceRequirements { requests, limits })
    }
}

// For aggregation of resource use, implement addition on normalised versions
impl Add for ResourceRequirements<f64> {
    type Output = ResourceRequirements<f64>;

    fn add(self, rhs: ResourceRequirements<f64>) -> ResourceRequirements<f64> {
        let requests = Resources {
            memory: self.requests.memory + rhs.requests.memory,
            cpu: self.requests.cpu + rhs.requests.cpu,
        };
        let limits = Resources {
            memory: self.limits.memory + rhs.limits.memory,
            cpu: self.limits.cpu + rhs.limits.cpu,
        };
        ResourceRequirements { requests, limits }
    }
}
impl AddAssign for ResourceRequirements<f64> {
    fn add_assign(&mut self, rhs: ResourceRequirements<f64>) {
        *self = self.clone() + rhs;
    }
}

impl Mul<u32> for ResourceRequirements<f64> {
    type Output = ResourceRequirements<f64>;

    fn mul(self, scalar: u32) -> ResourceRequirements<f64> {
        let requests = Resources {
            memory: self.requests.memory * f64::from(scalar),
            cpu: self.requests.cpu * f64::from(scalar),
        };
        let limits = Resources {
            memory: self.limits.memory * f64::from(scalar),
            cpu: self.limits.cpu * f64::from(scalar),
        };
        ResourceRequirements { requests, limits }
    }
}

/// Zero numericals used in computation.
/// Techncially this should be the std::num::Zero trait but it's unstable atm
impl Default for ResourceRequirements<f64> {
    fn default() -> Self {
        let requests = Resources {
            cpu: 0.0,
            memory: 0.0,
        };
        let limits = Resources {
            memory: 0.0,
            cpu: 0.0,
        };
        ResourceRequirements { requests, limits }
    }
}

impl ResourceRequirements<f64> {
    /// Convert to gigabytes and round to two decimals
    pub fn round(&mut self) {
        self.limits.memory = (self.limits.memory * 100.0 / (1024.0 * 1024.0 * 1024.0)).round() / 100.0;
        self.requests.memory = (self.requests.memory * 100.0 / (1024.0 * 1024.0 * 1024.0)).round() / 100.0;
        self.limits.cpu = (self.limits.cpu * 100.0).round() / 100.0;
        self.requests.cpu = (self.requests.cpu * 100.0).round() / 100.0;
    }
}

impl ResourceRequirements<String> {
    // TODO: look at config for limits?
    pub fn verify(&self) -> Result<()> {
        // (We can unwrap all the values as we assume implicit called!)
        let n = self.normalised()?;
        let req = &n.requests;
        let lim = &n.limits;

        // 1.1 limits >= requests
        if req.cpu > lim.cpu {
            bail!("Requested more CPU than what was limited");
        }
        if req.memory > lim.memory {
            bail!("Requested more memory than what was limited");
        }
        // 1.2 sanity numbers (based on c5.9xlarge)
        if req.cpu > 36.0 {
            bail!("Requested more than 36 cores");
        }
        if req.memory > 72.0 * 1024.0 * 1024.0 * 1024.0 {
            bail!("Requested more than 72 GB of memory");
        }
        if lim.cpu > 36.0 {
            bail!("CPU limit set to more than 36 cores");
        }
        if lim.memory > 72.0 * 1024.0 * 1024.0 * 1024.0 {
            bail!("Memory limit set to more than 72 GB of memory");
        }
        Ok(())
    }
}

/// Parse normal k8s memory/disk resource value into floats
///
/// Note that kubernetes insists on using upper case K for kilo against SI conventions:
/// > You can express memory as a plain integer or as a fixed-point integer using one of these suffixes: E, P, T, G, M, K. You can also use the power-of-two equivalents: Ei, Pi, Ti, Gi, Mi, Ki.
/// https://kubernetes.io/docs/concepts/configuration/manage-compute-resources-container/#meaning-of-memory
pub fn parse_memory(s: &str) -> Result<f64> {
    let digits = s
        .chars()
        .take_while(|ch| ch.is_digit(10) || *ch == '.')
        .collect::<String>();
    let unit = s
        .chars()
        .skip_while(|ch| ch.is_digit(10) || *ch == '.')
        .collect::<String>();
    let mut res: f64 = digits.parse()?;
    trace!("Parsed {} ({})", digits, unit);
    if unit == "Ki" {
        res *= 1024.0;
    } else if unit == "Mi" {
        res *= 1024.0 * 1024.0;
    } else if unit == "Gi" {
        res *= 1024.0 * 1024.0 * 1024.0;
    } else if unit == "Ti" {
        res *= 1024.0 * 1024.0 * 1024.0 * 1024.0;
    } else if unit == "Pi" {
        res *= 1024.0 * 1024.0 * 1024.0 * 1024.0 * 1024.0;
    } else if unit == "K" {
        res *= 1000.0;
    } else if unit == "M" {
        res *= 1000.0 * 1000.0;
    } else if unit == "G" {
        res *= 1000.0 * 1000.0 * 1000.0;
    } else if unit == "T" {
        res *= 1000.0 * 1000.0 * 1000.0 * 1000.0;
    } else if unit == "P" {
        res *= 1000.0 * 1000.0 * 1000.0 * 1000.0 * 1000.0;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} bytes", res);
    Ok(res)
}

// Parse normal k8s cpu resource values into floats
// We don't allow power of two variants here
fn parse_cpu(s: &str) -> Result<f64> {
    let digits = s
        .chars()
        .take_while(|ch| ch.is_digit(10) || *ch == '.')
        .collect::<String>();
    let unit = s
        .chars()
        .skip_while(|ch| ch.is_digit(10) || *ch == '.')
        .collect::<String>();
    let mut res: f64 = digits.parse()?;

    trace!("Parsed {} ({})", digits, unit);
    if unit == "m" {
        res /= 1000.0;
    } else if unit == "k" {
        res *= 1000.0;
    } else if unit != "" {
        bail!("Unknown unit {}", unit);
    }
    trace!("Returned {} cores", res);
    Ok(res)
}
