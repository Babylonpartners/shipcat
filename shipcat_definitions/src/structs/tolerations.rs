use super::Result;

/// Operator for a toleraton
#[derive(Serialize, Deserialize, Debug, Clone)]
pub enum Operator {
    Exists,
    Equal,
}

/// Effect of a toleration
#[derive(Serialize, Deserialize, Debug, Clone, PartialEq)]
pub enum Effect {
    NoSchedule,
    NoExecute,
    PreferNoSchedule,
}
impl Default for Effect {
    fn default() -> Self {
        Effect::NoSchedule
    }
}

/// Kubernetes Tolerations parameters for a service
#[derive(Serialize, Deserialize, Debug, Clone)]
pub struct Tolerations {
    /// What key does the toleration apply to?
    #[serde(default, skip_serializing_if = "Option::is_none")]
    key: Option<String>,
    /// Operator (Exists / Equal)
    pub operator: Operator,
    /// Value to match against (if Operator::Equal)
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub value: Option<String>,
    /// Effect this toleration has
    #[serde(default)]
    pub effect: Effect,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    /// How long to wait before being evicted (only for NoExecute)
    tolerationSeconds: Option<u32>,
}

impl Tolerations {
    pub fn verify(&self) -> Result<()> {
        match self.operator {
            Operator::Exists => assert!(
                self.value.is_none(),
                "must set tolerations.value when operator is Exists"
            ),
            Operator::Equal => assert!(
                self.value.is_some(),
                "cannot set tolerations.value when operator is Equal"
            ),
        }
        if self.effect != Effect::NoExecute && self.tolerationSeconds.is_some() {
            bail!("cannot set tolerations.tolerationSeconds unless effect is NoExecute");
        }
        Ok(())
    }
}
