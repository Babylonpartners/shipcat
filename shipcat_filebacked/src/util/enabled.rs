use std::collections::BTreeMap;

use merge::Merge;
use shipcat_definitions::Result;

use super::Build;

/// Enabled wraps a struct and adds an `enabled` field.
///
/// ```yaml
/// foo:
///     value: 3
/// bar:
///     enabled: false
///     value: 4
/// ```
///
/// is equivalent to
///
/// ```yaml
/// foo:
///     enabled: true
///     value: 3
/// bar: ~
/// ```
#[derive(Deserialize, Default, Clone, PartialEq, Merge)]
#[cfg_attr(test, derive(Debug, Copy))]
#[serde(default, deny_unknown_fields)]
pub struct Enabled<T: Merge> {
    pub enabled: Option<bool>,

    #[serde(flatten)]
    pub item: T,
}

/// Builds the inner struct unless enabled is explicitly false.
impl<S: Build<B, P> + Merge, B, P> Build<Option<B>, P> for Enabled<S> {
    fn build(self, params: &P) -> Result<Option<B>> {
        match self.enabled {
            Some(false) => Ok(None),
            _ => Ok(Some(self.item.build(params)?)),
        }
    }
}

/// EnabledMap is a map where each value is wrapped in an Enabled.
///
/// It can be built into a map which flattens the Enabled wrappers, so disabled values are excluded.
#[derive(Deserialize, Default, Clone, PartialEq)]
#[cfg_attr(test, derive(Debug))]
pub struct EnabledMap<K: Clone + std::hash::Hash + Ord, V: Clone + Default + Merge>(BTreeMap<K, Enabled<V>>);

impl<K: Clone + std::hash::Hash + Ord, V: Clone + Default + Merge> EnabledMap<K, V> {
    pub fn is_empty(&self) -> bool {
        self.0.is_empty()
    }
}

impl<K: Clone + std::hash::Hash + Ord, V: Clone + Default + Merge> IntoIterator for EnabledMap<K, V> {
    type IntoIter = std::collections::btree_map::IntoIter<K, Enabled<V>>;
    type Item = (K, Enabled<V>);

    fn into_iter(self) -> Self::IntoIter {
        self.0.into_iter()
    }
}

impl<K, VS, VB, P> Build<BTreeMap<K, VB>, P> for EnabledMap<K, VS>
where
    K: Clone + std::hash::Hash + Ord,
    VS: Build<VB, P> + Clone + Default + Merge,
{
    fn build(self, params: &P) -> Result<BTreeMap<K, VB>> {
        let mut built = BTreeMap::new();
        for (k, vs) in self.0 {
            if let Some(vb) = vs.build(params)? {
                built.insert(k, vb);
            }
        }
        Ok(built)
    }
}

impl<K, V> Merge for EnabledMap<K, V>
where
    K: Clone + std::hash::Hash + Ord,
    V: Clone + Default + Merge,
{
    fn merge(self, other: Self) -> Self {
        let Self(mut merged) = self;
        for (k, v) in other.0 {
            let m = if let Some(s) = merged.remove(&k) {
                s.merge(v)
            } else {
                v
            };
            merged.insert(k, m);
        }
        Self(merged)
    }
}

#[cfg(test)]
mod tests {
    use maplit::btreemap;

    use merge::Merge;
    use shipcat_definitions::Result;
    use std::collections::BTreeMap;

    use super::{Build, Enabled, EnabledMap};

    #[derive(Deserialize, Default, Clone)]
    struct Item(&'static str);

    impl Build<String, u32> for Item {
        fn build(self, params: &u32) -> Result<String> {
            Ok(format!("{}/{}", self.0, params))
        }
    }

    impl Merge for Item {
        fn merge(self, _: Self) -> Self {
            panic!("Not implemented: unused by tests");
        }
    }

    #[test]
    fn build() {
        let source = Enabled {
            enabled: None,
            item: Item("foo"),
        };
        assert_eq!(source.build(&1).unwrap(), Some("foo/1".into()));

        let source = Enabled {
            enabled: Some(true),
            item: Item("bar"),
        };
        assert_eq!(source.build(&2).unwrap(), Some("bar/2".into()));

        let source = Enabled {
            enabled: Some(false),
            item: Item("blort"),
        };
        assert_eq!(source.build(&3).unwrap(), None);
    }

    #[test]
    fn build_map() {
        let mut source = BTreeMap::new();
        source.insert("a", Enabled {
            enabled: None,
            item: Item("foo"),
        });
        source.insert("b", Enabled {
            enabled: Some(true),
            item: Item("bar"),
        });
        source.insert("c", Enabled {
            enabled: Some(false),
            item: Item("blort"),
        });
        let source = EnabledMap(source);

        assert_eq!(source.build(&1).unwrap(), btreemap! {
            "a" => "foo/1".into(),
            "b" => "bar/1".into(),
        });
    }

    #[test]
    fn merge() {
        let empty = Enabled::<Option<u32>> {
            enabled: None,
            item: None,
        };
        let full = Enabled {
            enabled: Some(true),
            item: Some(2),
        };
        let partial1 = Enabled {
            enabled: Some(false),
            item: None,
        };
        let partial2 = Enabled {
            enabled: None,
            item: Some(4),
        };

        // x.merge(x) == x
        assert_eq!(empty.merge(empty), empty);
        assert_eq!(full.merge(full), full);
        assert_eq!(partial1.merge(partial1), partial1);
        assert_eq!(partial2.merge(partial2), partial2);

        // empty.merge(x) == x
        assert_eq!(empty.merge(full), full);
        assert_eq!(empty.merge(partial1), partial1);
        assert_eq!(empty.merge(partial2), partial2);

        // x.merge(empty) == x
        assert_eq!(full.merge(empty), full);
        assert_eq!(partial1.merge(empty), partial1);
        assert_eq!(partial2.merge(empty), partial2);

        // x.merge(full) == full
        assert_eq!(partial1.merge(full), full);
        assert_eq!(partial2.merge(full), full);

        assert_eq!(full.merge(partial1), Enabled {
            enabled: Some(false),
            item: Some(2),
        });
        assert_eq!(full.merge(partial2), Enabled {
            enabled: Some(true),
            item: Some(4),
        });
        assert_eq!(partial1.merge(partial2), Enabled {
            enabled: Some(false),
            item: Some(4),
        });
        assert_eq!(partial2.merge(partial1), Enabled {
            enabled: Some(false),
            item: Some(4),
        });
    }

    #[test]
    fn merge_map() {
        let full = EnabledMap(btreemap! {
            "foo" => Enabled {
                enabled: Some(false),
                item: Some(1),
            },
            "bar" => Enabled {
                enabled: Some(true),
                item: Some(2),
            },
            "blort" => Enabled {
                enabled: Some(true),
                item: Some(3),
            }
        });
        let partial1 = EnabledMap(btreemap! {
            "foo" => Enabled::<Option<u32>> {
                enabled: Some(true),
                item: None,
            },
            "bar" => Enabled {
                enabled: Some(false),
                item: Some(20),
            },
        });
        let empty = EnabledMap(BTreeMap::new());

        // x.merge(x) == x
        assert_eq!(full.clone().merge(full.clone()), full);
        assert_eq!(partial1.clone().merge(partial1.clone()), partial1);
        assert_eq!(empty.clone().merge(empty.clone()), empty);

        // empty.merge(x) == x
        assert_eq!(empty.clone().merge(full.clone()), full);
        assert_eq!(empty.clone().merge(partial1.clone()), partial1);

        // x.merge(empty) == x
        assert_eq!(full.clone().merge(empty.clone()), full);
        assert_eq!(partial1.clone().merge(empty.clone()), partial1);

        // x.merge(full) == full
        assert_eq!(partial1.clone().merge(full.clone()), full);

        assert_eq!(
            full.clone().merge(partial1.clone()),
            EnabledMap(btreemap! {
                "foo" => Enabled {
                    enabled: Some(true), // from partial
                    item: Some(1), // from full
                },
                "bar" => Enabled {
                    enabled: Some(false), // from partial
                    item: Some(20), // from partial
                },
                "blort" => Enabled {
                    enabled: Some(true),
                    item: Some(3),
                },
            })
        )
    }
}
