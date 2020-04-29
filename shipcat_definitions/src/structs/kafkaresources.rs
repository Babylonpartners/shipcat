use super::Result;
use regex::Regex;
use std::collections::BTreeMap;

#[derive(Serialize, Deserialize, Clone, Debug)]
pub struct KafkaTopics {
    pub name: String,

    #[serde(default)]
    pub partitions: i32,

    #[serde(default)]
    pub replicas: i32,

    #[serde(default, skip_serializing_if = "BTreeMap::is_empty")]
    pub config: BTreeMap<String, String>,
}

/// Resource Types relating to a Kafka ACL to be applied onto a resource,
/// values derived from the Strimzi Kafka User Custom Resource Definition
/// [Strimzi Kafka User CRD ](https://github.com/strimzi/strimzi-kafka-operator/blob/master/install/user-operator/04-Crd-kafkauser.yaml)
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum KafkaUserResourceType {
    Topic,
    Group,
    Cluster,
    TransactionalId,
}

/// Operations relating to a Kafka ACL to be applied onto a resource,
/// values derived from the Strimzi Kafka User Custom Resource Definition
/// [Strimzi Kafka User CRD ](https://github.com/strimzi/strimzi-kafka-operator/blob/master/install/user-operator/04-Crd-kafkauser.yaml)
#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "PascalCase")]
pub enum KafkaUserOperation {
    Read,
    Write,
    Create,
    Delete,
    Alter,
    Describe,
    ClusterAction,
    AlterConfigs,
    DescribeConfigs,
    IdempotentWrite,
    All,
}

#[derive(Serialize, Deserialize, Clone, Debug)]
#[serde(rename_all = "camelCase")]
pub enum KafkaUserPatternType {
    Literal,
    Prefix,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields, rename_all = "camelCase")]
pub struct AclDefinition {
    pub resource_name: String,
    pub resource_type: Option<KafkaUserResourceType>,
    pub pattern_type: Option<KafkaUserPatternType>,
    pub operation: Option<KafkaUserOperation>,
    pub host: String,
}

#[derive(Default, Serialize, Deserialize, Clone, Debug)]
#[serde(deny_unknown_fields)]
pub struct KafkaUsers {
    pub name: String,
    pub acls: Vec<AclDefinition>,
}


#[derive(Debug, Default, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct KafkaResources {
    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub topics: Vec<KafkaTopics>,

    #[serde(default, skip_serializing_if = "Vec::is_empty")]
    pub users: Vec<KafkaUsers>,
}

impl KafkaResources {
    const K8S_NAMING_PATTERN: &'static str = r"^[0-9a-z\-]{1,63}$";

    fn is_VALID_K8S_NAME(value: &str) -> bool {
        Regex::new(KafkaResources::K8S_NAMING_PATTERN)
            .unwrap()
            .is_match(value)
    }

    fn is_VALID_PARTITIONS(value: &i32) -> bool {
        let partition_range = 1..10000;
        partition_range.contains(value)
    }

    fn is_VALID_REPLICAS(value: &i32) -> bool {
        let replica_range = 1..32767;
        replica_range.contains(value)
    }

    pub fn verify(&self) -> Result<()> {
        let mut failed_topics = vec![];
        let mut failed_partitions = vec![];
        let mut failed_replicas = vec![];

        for topic in &self.topics {
            if !KafkaResources::is_VALID_K8S_NAME(&topic.name) {
                failed_topics.push(&topic.name);
                // error!("Topic Name \"{}\" is not a valid Kafka Topic Name", &topic.name);
            }
            if !KafkaResources::is_VALID_PARTITIONS(&topic.partitions) {
                failed_partitions.push(&topic.partitions);
            }
            if !KafkaResources::is_VALID_REPLICAS(&topic.replicas) {
                failed_replicas.push(&topic.replicas);
            }
        }

        if failed_topics.is_empty() {
            Ok(())
        } else {
            bail!("invalid topic name(s): {:?}", failed_topics);
        }

        // ignore these tests for now, cant impl a default value when value is empty
        // as its region / context specific
        // if failed_topics.is_empty() && failed_partitions.is_empty() && failed_replicas.is_empty() {
        //     Ok(())
        // } else if !failed_topics.is_empty() {
        //     bail!("invalid topic name(s): {:?}", failed_topics);
        // } else if !failed_partitions.is_empty() {
        //     bail!("invalid partition integer: {:?}", failed_partitions);
        // } else if !failed_replicas.is_empty() {
        //     bail!("invalid replica integer: {:?}", failed_replicas);
        // } else {
        //     bail!("unknown KafkaResource Error");
        // }
    }
}

#[cfg(test)]
mod tests {
    use super::KafkaResources;


    fn validKafkaResource(input: &str) -> KafkaResources {
        let kr: KafkaResources = serde_yaml::from_str(input).unwrap();
        kr
    }

    #[test]
    fn verifies_if_valid_resource() {
        let VALID_KAFKA_RESOURCE = r###"
    topics:
    - name: my-topic
      partitions: 1
      replicas: 3
      config:
        retention.ms: 604800000
        segment.bytes: 1073741824
    users:
    - name: my-user
      acls:
      - resourceName: testtopic
        resourceType: topic
        patternType: literal
        operation: Write
        host: "*"
      - resourceName: testtopic
        resourceType: topic
        patternType: literal
        operation: Read
        host: "*""###;
        let kr = validKafkaResource(&VALID_KAFKA_RESOURCE);
        kr.verify().unwrap();
    }

    #[test]
    fn verifies_if_invalid_resource() {
        let INVALID_KAFKA_RESOURCE = r###"
    topics:
    - name: my_topic
      partitions: 1
      replicas: 3
      config:
        retention.ms: 604800000
        segment.bytes: 1073741824
    users:
    - name: my-user
      acls:
      - resourceName: testtopic
        resourceType: topic
        patternType: literal
        operation: Write
        host: "*"
      - resourceName: testtopic
        resourceType: topic
        patternType: literal
        operation: Read
        host: "*""###;
        let kr = validKafkaResource(&INVALID_KAFKA_RESOURCE);
        kr.verify().unwrap_err();
    }
}
