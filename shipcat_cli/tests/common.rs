use semver::Version;
use std::{collections::BTreeSet, env, fs, path::Path, sync::Once};

static START: Once = Once::new();

macro_rules! btree_set {
    ( $( $x:expr ),* ) => {
        {
            let mut set = BTreeSet::new();
            $(
                set.insert($x);
            )*
            set
        }
    };
}

/// Set cwd to tests directory to be able to test manifest functionality
///
/// The tests directory provides a couple of fake services for verification
pub fn setup() {
    START.call_once(|| {
        let pwd = env::current_dir().unwrap();
        let pth = fs::canonicalize(Path::new(&pwd).join("..").join("tests")).unwrap();
        env::set_var("SHIPCAT_MANIFEST_DIR", pth.clone());
        // loggerv::Logger::new()
        //    .verbosity(1) // TODO: filter tokio/hyper and bump
        //    .module_path(true)
        //    .line_numbers(true)
        //    .init()
        //    .unwrap();
        // TODO: stop creating multiple reqwest clients in tests, might not be safe
        println!("Initializing tests - using testdir {}", pth.display());
        assert!(env::set_current_dir(pth).is_ok());
    });
}

use shipcat_definitions::{Config, ConfigState, Environment}; // Product

#[tokio::test]
async fn config_test() {
    setup();
    assert!(Config::read().await.is_ok());
    assert!(Config::new(ConfigState::Base, "dev-uk").await.is_ok());
    let filteredcfg = Config::new(ConfigState::Filtered, "dev-uk");
    let (conf, _region) = filteredcfg.await.unwrap(); // better to unwrap and get full trace
    assert!(Config::new(ConfigState::File, "dev-uk").await.is_err());
    assert!(conf.print().is_ok());
}

#[tokio::test]
async fn config_cr_settings_test() {
    setup();
    Config::read().await.unwrap(); // iof assert!(Config::read().is_ok());
    Config::new(ConfigState::Base, "dev-ops").await.unwrap();
    let gbcfg = Config::new(ConfigState::UnionisedBase, "dev-ops");
    let (conf, _region) = gbcfg.await.unwrap();
    assert!(conf.print().is_ok());
}

#[tokio::test]
async fn config_defaults_test() {
    setup();
    let (conf, reg) = Config::new(ConfigState::Base, "dev-uk").await.unwrap();

    // -- Slack channels --

    // fake-ask gets default for 'observability'
    let mfdefault = shipcat_filebacked::load_manifest("fake-ask", &conf, &reg)
        .await
        .unwrap()
        .complete(&reg)
        .await
        .unwrap();
    let metadata = mfdefault.metadata.unwrap();
    assert_eq!(*metadata.support.unwrap(), "CA04UJ8S0"); // from teams.yml
    assert_eq!(*metadata.notifications.unwrap(), "CA04UJ8S0");

    // fake-storage overrides for 'someteam'
    let mfoverride = shipcat_filebacked::load_manifest("fake-storage", &conf, &reg)
        .await
        .unwrap()
        .complete(&reg)
        .await
        .unwrap();
    let metadata = mfoverride.metadata.unwrap();
    assert_eq!(*metadata.support.unwrap(), "#dev-platform-override");
    assert_eq!(*metadata.notifications.unwrap(), "#dev-platform-notif-override");
}

use shipcat::get;
#[tokio::test]
async fn getters() {
    setup();
    let (conf, reg) = Config::new(ConfigState::Base, "dev-uk").await.unwrap();
    let vers = get::versions(&conf, &reg).await.unwrap();
    assert_eq!(vers.len(), 1); // only one of the services has a version
    assert_eq!(vers["fake-ask"], Version::new(1, 6, 0));

    let imgs = get::images(&conf, &reg).await.unwrap();
    assert_eq!(imgs.len(), 2); // every service gets an image
    assert_eq!(imgs["fake-ask"], "quay.io/babylonhealth/fake-ask");
    assert_eq!(imgs["fake-storage"], "nginx");
}

#[tokio::test]
async fn clusterinfo() {
    setup();
    // clusterinfo must be correct in its resolution!
    // test all cases
    // NB: needs a base config to be able to verify region/cluster constraints
    let conf = Config::read().await.unwrap();

    assert!(get::clusterinfo(&conf, "preproduk-blue", Some("preproduk-blue")).is_ok());
    assert!(get::clusterinfo(&conf, "preproduk-green", None).is_err()); // ambiguous
    assert!(get::clusterinfo(&conf, "preprod-uk", None).is_err()); // ambiguous

    let blue = get::clusterinfo(&conf, "preprod-uk", Some("preproduk-blue")).unwrap();
    assert_eq!(blue.region, "preprod-uk"); // correctly resolved

    assert!(get::clusterinfo(&conf, "dev-global", None).is_ok());
    let devglob = get::clusterinfo(&conf, "dev-global", None).unwrap();
    assert_eq!(devglob.region, "dev-global");
    assert_eq!(devglob.cluster, "kops-global")
}

#[tokio::test]
async fn get_codeowners() {
    setup();
    let conf = Config::read().await.unwrap();
    let cos = get::codeowners(&conf).await.unwrap();

    assert_eq!(cos.len(), 4); // serivces with team admins get a listing
    assert_eq!(cos[1], "services/fake-ask/* @babylonhealth/o11y @clux");
}

#[tokio::test]
async fn manifest_test() {
    setup();
    let (conf, reg) = Config::new(ConfigState::Base, "dev-uk").await.unwrap();
    let mfread = shipcat_filebacked::load_manifest("fake-storage", &conf, &reg).await;
    assert!(mfread.is_ok());
    let mfbase = mfread.unwrap();
    let mfres = mfbase.complete(&reg).await;
    assert!(mfres.is_ok());
    let mf = mfres.unwrap();

    // verify datahandling implicits
    let dh = mf.dataHandling.unwrap();
    let s3 = dh.stores[0].clone();
    assert!(s3.encrypted.unwrap());
    assert_eq!(s3.fields[0].encrypted.unwrap(), false); // overridden
    assert_eq!(s3.fields[1].encrypted.unwrap(), true); // cascaded
    assert_eq!(s3.fields[0].keyRotator, None); // not set either place
    assert_eq!(s3.fields[1].keyRotator, Some("2w".into())); // field value
}

#[tokio::test]
async fn templating_test() {
    setup();
    let (conf, reg) = Config::new(ConfigState::Base, "dev-uk").await.unwrap();
    let mf = shipcat_filebacked::load_manifest("fake-ask", &conf, &reg)
        .await
        .unwrap()
        .complete(&reg)
        .await
        .unwrap();

    // verify templating
    let env = mf.env.plain;
    assert_eq!(&env["CORE_URL"], "https://woot.com/somesvc");
    // check values from Config - one plain, one as_secret
    assert_eq!(&env["CLIENT_ID"], "FAKEASKID");

    assert_eq!(mf.env.secrets, btree_set!["FAKE_SECRET".to_string()]);

    // verify environment defaults
    assert_eq!(&env["GLOBAL_EVAR"], "indeed");
    // verify environment overrides
    assert_eq!(&env["EXTRA_URL"], "https://blah/extra-svc/");
    assert_eq!(&env["MODE"], "development");

    // verify sidecar templating
    let redis = &mf.sidecars[0];
    assert_eq!(&redis.env.plain["STATIC_VALUE"], "static");
    assert_eq!(
        redis.env.plain["CORE_URL"],
        "https://woot.com/somesvc".to_string()
    );
    assert_eq!(redis.env.secrets, btree_set![
        "FAKE_NUMBER".to_string(),
        "FAKE_SECRET".to_string()
    ]);

    // verify worker templating
    let w = &mf.workers[0];
    assert_eq!(&w.container.env.plain["URL"], "https://woot.com/worker");
    assert_eq!(w.container.env.secrets, BTreeSet::new());

    // verify cron job templating
    let c = &mf.cronJobs[0];
    assert_eq!(&c.container.env.plain["URL"], "https://woot.com/cronjob");
    assert_eq!(c.container.env.secrets, BTreeSet::new());

    // verify secrets
    let sec = mf.secrets;
    assert_eq!(&sec["FAKE_SECRET"], "hello"); // NB: ACTUALLY IN_VAULT
    assert_eq!(&sec["FAKE_NUMBER"], "-2"); // NB: ACTUALLY IN_VAULT

    let configs = mf.configs.clone().unwrap();
    let configini = configs.files[0].clone();
    let cfgtpl = configini.value.unwrap();
    print!("{:?}", cfgtpl);
    assert!(cfgtpl.contains("CORE=https://woot.com/somesvc"));
    assert!(cfgtpl.contains("CLIENT_ID"));
    assert!(cfgtpl.contains("CLIENT_ID=FAKEASKID"));
}

#[tokio::test]
async fn vault_policy_test() {
    setup();
    let (conf, reg) = Config::new(ConfigState::Base, "dev-uk").await.unwrap();
    let policy = shipcat::get::vaultpolicy(&conf, &reg, "observability")
        .await
        .unwrap();

    println!("got dev policy for observability as {}", policy);
    let expected_deny = r#"path "sys/*" {
  policy = "deny"
}"#;
    let expected_list = r#"path "secret/*" {
  capabilities = ["list"]
}"#;
    let expected_access = r#"path "secret/dev-uk/fake-ask/*" {
  capabilities = ["create", "read", "update", "delete", "list"]
}"#;

    assert!(policy.contains(expected_deny));
    assert!(policy.contains(expected_list));
    assert!(policy.contains(expected_access));

    // prod should be stricter:
    let mut fakereg = reg.clone();
    fakereg.environment = Environment::Prod;
    let strict_policy = shipcat::get::vaultpolicy(&conf, &fakereg, "observability")
        .await
        .unwrap();

    let expected_strict_access = r#"path "secret/dev-uk/fake-ask/*" {
  capabilities = ["create", "list"]
}"#;
    assert!(strict_policy.contains(expected_strict_access));
}
