#[macro_use] extern crate clap;
#[macro_use] extern crate log;

use std::str::FromStr;
use shipcat::*;
use clap::{Arg, App, AppSettings, SubCommand, ArgMatches, Shell};
use std::process;

fn print_error_debug(e: &Error) {
    use std::env;
    // print causes of error if present
    if let Ok(_) = env::var("CIRCLECI") {
        // https://github.com/clux/muslrust/issues/42
        // only print debug implementation rather than unwinding
        warn!("{:?}", e);
    } else {
        // normal case - unwind the error chain
        for e in e.iter().skip(1) {
            warn!("caused by: {}", e);
        }
    }
}

fn build_cli() -> App<'static, 'static> {
    App::new("shipcat")
        .version(crate_version!())
        .setting(AppSettings::VersionlessSubcommands)
        .setting(AppSettings::SubcommandRequiredElseHelp)
        .setting(AppSettings::ColoredHelp)
        .setting(AppSettings::DeriveDisplayOrder)
        .global_settings(&[AppSettings::ColoredHelp])
        .about("Deploy right meow")
        .arg(Arg::with_name("verbose")
            .short("v")
            .multiple(true)
            .global(true)
            .help("Increase verbosity"))
        .arg(Arg::with_name("debug")
            .short("d")
            .long("debug")
            .global(true)
            .help("Adds line numbers to log statements"))
        .arg(Arg::with_name("region")
                .short("r")
                .long("region")
                .takes_value(true)
                .global(true)
                .help("Region to use (dev-uk, staging-uk, prod-uk)"))
        .subcommand(SubCommand::with_name("debug")
            .about("Get debug information about a release running in a cluster")
            .arg(Arg::with_name("service")
                .required(true)
                .help("Service name")))

        .subcommand(SubCommand::with_name("completions")
            .about("Generate autocompletion script for shipcat for the specified shell")
            .usage("This can be source using: $ source <(shipcat completions bash)")
            .arg(Arg::with_name("shell")
                .required(true)
                .possible_values(&Shell::variants())
                .help("Shell to generate completions for (zsh or bash)")))

        .subcommand(SubCommand::with_name("shell")
            .about("Shell into pods for a service described in a manifest")
            .arg(Arg::with_name("pod")
                .takes_value(true)
                .short("p")
                .long("pod")
                .help("Pod number - otherwise tries first"))
            .arg(Arg::with_name("service")
                .required(true)
                .help("Service name"))
            .setting(AppSettings::TrailingVarArg)
            .arg(Arg::with_name("cmd").multiple(true)))

        .subcommand(SubCommand::with_name("port-forward")
            .about("Port forwards a service to localhost")
            .arg(Arg::with_name("service")
                .required(true)
                .help("Service name")))

        .subcommand(SubCommand::with_name("slack")
            .arg(Arg::with_name("url")
                .short("u")
                .long("url")
                .takes_value(true)
                .help("url|description to link to at the end of the message"))
            .arg(Arg::with_name("message")
                .required(true)
                .multiple(true))
            .arg(Arg::with_name("color")
                .short("c")
                .long("color")
                .takes_value(true))
            .setting(AppSettings::TrailingVarArg)
            .about("Post message to slack"))

        .subcommand(SubCommand::with_name("validate")
              .arg(Arg::with_name("services")
                .required(true)
                .multiple(true)
                .help("Service names to validate"))
              .arg(Arg::with_name("secrets")
                .short("s")
                .long("secrets")
                .help("Verifies secrets exist everywhere"))
            .arg(Arg::with_name("skip-version-check")
                .long("skip-version-check")
                .help("Skip checking if the current region is supported by this Shipcat version"))
              .about("Validate the shipcat manifest"))

        .subcommand(SubCommand::with_name("verify")
            .about("Verify all manifests of a region"))

        .subcommand(SubCommand::with_name("secret")
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .subcommand(SubCommand::with_name("verify-region")
                .arg(Arg::with_name("services")
                    .long("services")
                    .takes_value(true)
                    .required(false)
                    .conflicts_with("git")
                    .help("Explicit services to validate (comma separated)"))
                .arg(Arg::with_name("git")
                    .long("git")
                    .conflicts_with("services")
                    .help("Checks services changed in git only"))
                .arg(Arg::with_name("regions")
                    .required(true)
                    .multiple(true)
                    .help("Regions to validate all enabled services for"))
                .about("Verify existence of secrets for entire regions"))
            .about("Secret interaction"))

        .subcommand(SubCommand::with_name("gdpr")
              .arg(Arg::with_name("service")
                .help("Service names to show"))
              .about("Reduce data handling structs"))

        .subcommand(SubCommand::with_name("get")
              .arg(Arg::with_name("cluster")
                .short("c")
                .long("cluster")
                .takes_value(true)
                .help("Specific cluster to check (if relevant)"))
              .about("Reduce encoded info")
              .subcommand(SubCommand::with_name("images")
                .help("Reduce encoded image info"))
              .subcommand(SubCommand::with_name("resources")
                .help("Reduce encoded resouce requests and limits"))
              .subcommand(SubCommand::with_name("apistatus")
                .help("Reduce encoded API info"))
              .subcommand(SubCommand::with_name("codeowners")
                .help("Generate CODEOWNERS syntax for manifests based on team ownership"))
              .subcommand(SubCommand::with_name("vault-policy")
                .arg(Arg::with_name("team")
                  .required(true)
                  .help("Team to generate the policy for"))
                .help("Generate vault-policies syntax for a region based on team ownership"))
              .subcommand(SubCommand::with_name("clusterinfo")
                .help("Reduce encoded cluster information"))
              .subcommand(SubCommand::with_name("vault-url")
                .help("Get the vault-url in a region"))
              .subcommand(SubCommand::with_name("versions")
                .help("Reduce encoded version info")))
        // kong helper
        .subcommand(SubCommand::with_name("kong")
            .about("Generate Kong config")
            .arg(Arg::with_name("crd")
                .long("crd")
                .help("Produce an experimental custom resource values for this kubernetes region"))
            .subcommand(SubCommand::with_name("config-url")
                .help("Generate Kong config URL")))
        // Statuscake helper
        .subcommand(SubCommand::with_name("statuscake")
            .about("Generate Statuscake config"))
        // dependency graphing
        .subcommand(SubCommand::with_name("graph")
              .arg(Arg::with_name("service")
                .help("Service name to graph around"))
              .arg(Arg::with_name("dot")
                .long("dot")
                .help("Generate dot output for graphviz"))
              .arg(Arg::with_name("reverse")
                .long("reverse")
                .help("Generate reverse dependencies for a service"))
              .about("Graph the dependencies of a service"))
        // cluster admin operations
        .subcommand(SubCommand::with_name("cluster")
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .about("Perform cluster level recovery / reconcilation commands")
            .subcommand(SubCommand::with_name("diff")
                // Use RAYON_NUM_THREADS to parallelize
                .about("Diff all services against the a region"))
            .subcommand(SubCommand::with_name("crd")
                .arg(Arg::with_name("num-jobs")
                    .short("j")
                    .long("num-jobs")
                    .takes_value(true)
                    .help("Number of worker threads used"))
                .subcommand(SubCommand::with_name("reconcile")
                    .about("Reconcile shipcat custom resource definitions with local state")))
            .subcommand(SubCommand::with_name("vault-policy")
                .arg(Arg::with_name("num-jobs")
                    .short("j")
                    .long("num-jobs")
                    .takes_value(true)
                    .help("Number of worker threads used"))
                .subcommand(SubCommand::with_name("reconcile")
                    .about("Reconcile vault policies with manifest state"))))
        // all the listers (hidden from cli output)
        .subcommand(SubCommand::with_name("list-regions")
            .setting(AppSettings::Hidden)
            .about("list supported regions/clusters"))
        .subcommand(SubCommand::with_name("list-locations")
            .setting(AppSettings::Hidden)
            .about("list supported product locations"))
        .subcommand(SubCommand::with_name("list-services")
            .setting(AppSettings::Hidden)
            .about("list supported services for a specified"))
        .subcommand(SubCommand::with_name("list-products")
            .setting(AppSettings::Hidden)
            .arg(Arg::with_name("location")
                .required(true)
                .help("Location to filter on"))
            .about("list supported products"))

        // new service subcommands (absorbing some service manifest responsibility from helm/validate cmds)
        .subcommand(SubCommand::with_name("status")
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to check"))
              .about("Show kubernetes status for all the resources for a service"))

        .subcommand(SubCommand::with_name("version")
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to check"))
              .about("Ask kubernetes for the current running version of a service"))

        .subcommand(SubCommand::with_name("crd")
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to generate crd for"))
              .about("Generate the kube equivalent ShipcatManifest CRD"))

        .subcommand(SubCommand::with_name("values")
              .arg(Arg::with_name("secrets")
                .short("s")
                .long("secrets")
                .help("Use actual secrets from vault"))
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to generate values for"))
              .about("Generate the completed service manifest that will be passed to the helm chart"))
        .subcommand(SubCommand::with_name("template")
              .arg(Arg::with_name("secrets")
                .short("s")
                .long("secrets")
                .help("Use actual secrets from vault"))
              .arg(Arg::with_name("current")
                .short("k")
                .long("current")
                .help("Use uids and versions from the current kubernetes shipcatmanifest"))
              .arg(Arg::with_name("check")
                .short("c")
                .long("check")
                .help("Check the validity of the template"))
              .arg(Arg::with_name("tag")
                .long("tag")
                .short("t")
                .takes_value(true)
                .help("Image version to override (useful when validating)"))
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to generate kube yaml for"))
            .about("Generate kube yaml for a service (through helm)"))
        .subcommand(SubCommand::with_name("apply")
              .arg(Arg::with_name("tag")
                .long("tag")
                .short("t")
                .takes_value(true)
                .help("Image version to deploy"))
              .arg(Arg::with_name("no-wait")
                    .long("no-wait")
                    .help("Do not wait for service timeout"))
              .arg(Arg::with_name("force")
                    .long("force")
                    .help("Apply template even if no changes are detected"))
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to apply"))
            .about("Apply a service's configuration in kubernetes (through helm)"))

        .subcommand(SubCommand::with_name("restart")
              .arg(Arg::with_name("no-wait")
                    .long("no-wait")
                    .help("Do not wait for service timeout"))
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to restart"))
            .about("Restart a deployment rollout to restart all pods safely"))

        .subcommand(SubCommand::with_name("env")
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to generate an environment for"))
              .arg(Arg::with_name("secrets")
                .short("s")
                .long("secrets")
                .help("Use actual secrets from vault"))
              .about("Show env vars in a format that can be sourced in a shell"))

        .subcommand(SubCommand::with_name("diff")
              .arg(Arg::with_name("git")
                .long("git")
                .global(true)
                .help("Comparing with master using a temporary git stash and git checkout"))
              .arg(Arg::with_name("with-region")
                .long("with-region")
                .global(true)
                .takes_value(true)
                .conflicts_with("git")
                .conflicts_with("crd")
                .conflicts_with("helm")
                .help("Comparing with the same service in a different region"))
              .arg(Arg::with_name("helm")
                .long("helm")
                .global(true)
                .conflicts_with("git")
                .conflicts_with("crd")
                .help("Comparing using helm-diff plugin"))
              .arg(Arg::with_name("tag")
                .long("tag")
                .short("t")
                .takes_value(true)
                .help("Image version to deploy"))
              .arg(Arg::with_name("service")
                .required(true)
                .help("Service to be diffed"))
              .arg(Arg::with_name("crd")
                .long("crd")
                .help("Compare the shipcatmanifest crd output instead of the full kube yaml"))
              .arg(Arg::with_name("current")
                .short("k")
                .long("current")
                .help("Use uids and versions from the current kubernetes shipcatmanifest"))
              .arg(Arg::with_name("minify")
                .short("m")
                .long("minify")
                .help("Minify the diff context"))
              .arg(Arg::with_name("obfuscate")
                .long("obfuscate")
                .requires("secrets")
                .help("Obfuscate secrets in the diff"))
              .arg(Arg::with_name("secrets")
                .long("secrets")
                .short("s")
                .help("Fetch secrets before comparing")
                .conflicts_with("git")
                .conflicts_with("crd"))
            .about("Diff a service's yaml output against master or kubernetes"))

        // config
        .subcommand(SubCommand::with_name("config")
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .about("Run interactions on shipcat.conf")
            .subcommand(SubCommand::with_name("show")
                .about("Show the config"))
            .subcommand(SubCommand::with_name("crd")
                .about("Show the config in crd form for a region"))
            .subcommand(SubCommand::with_name("verify")
                .about("Verify the parsed config")))

        // products
        .subcommand(SubCommand::with_name("product")
            .setting(AppSettings::SubcommandRequiredElseHelp)
            .about("Run product interactions across manifests")
            .subcommand(SubCommand::with_name("show")
                .arg(Arg::with_name("product")
                    .required(true)
                    .help("Product name"))
                .arg(Arg::with_name("location")
                    .required(true)
                    .help("Location name"))
                .about("Show product information"))
            .subcommand(SubCommand::with_name("verify")
                .arg(Arg::with_name("products")
                    .required(true)
                    .help("Product names"))
                .arg(Arg::with_name("location")
                    .long("location")
                    .short("l")
                    .takes_value(true)
                    .required(false)
                    .help("Location name"))
                .about("Verify product manifests")))

        .subcommand(SubCommand::with_name("login")
            .about("Login to a region (using teleport if possible)")
            .arg(Arg::with_name("force")
                .long("force")
                .short("f")
                .help("Remove the old tsh state file to force a login"))
            )
}

fn main() {
    let app = build_cli();
    let args = app.get_matches();

    // completions handling first
    if let Some(a) = args.subcommand_matches("completions") {
        let sh = Shell::from_str(a.value_of("shell").unwrap()).unwrap();
        build_cli().gen_completions_to("shipcat", sh, &mut std::io::stdout());
        process::exit(0);
    }

    let name = args.subcommand_name().unwrap();
    let _ = run(&args).map_err(|e| {
        error!("{} error: {}", name, e);
        print_error_debug(&e);
        process::exit(1);
    });
    process::exit(0);
}

fn run(args: &ArgMatches) -> Result<()> {
    // initialise deps and set log default - always show INFO messages (+1)
    loggerv::Logger::new()
        .verbosity(args.occurrences_of("verbose") + 1)
        .module_path(true) // may need cargo clean's if it fails..
        .line_numbers(args.is_present("debug"))
        .output(&log::Level::Info, loggerv::Output::Stderr)
        .output(&log::Level::Debug, loggerv::Output::Stderr)
        .output(&log::Level::Trace, loggerv::Output::Stderr)
        .init()
        .unwrap();
    shipcat::init()?;

    // Ignore SIGPIPE errors to avoid having to use let _ = write! everywhere
    // See https://github.com/rust-lang/rust/issues/46016
    unsafe {
        libc::signal(libc::SIGPIPE, libc::SIG_DFL);
    }

    // Dispatch arguments to internal handlers. Pass on handled result.
    dispatch_commands(args)
}

/// Create a config for a region
///
/// Resolves an optional "region" Arg or falls back to kube context.
/// This is the ONLY user of kubectl::current_context for sanity.
/// If the CLI entrypoint does not need a region-wide config, do not use this.
fn resolve_config(args: &ArgMatches, ct: ConfigType) -> Result<(Config, Region)> {
    let regionguess = if let Some(r) = args.value_of("region") {
        r.into()
    } else {
        kubectl::current_context()?
    };
    let res = Config::new(ct, &regionguess)?;
    Ok(res)
}

fn void<T>(_x: T) {} // helper so that dispatch_commands can return Result<()>

/// Dispatch clap arguments to shipcat handlers
///
/// A boring and somewhat error-prone "if-x-then-fnx dance". We are relying on types
/// in the dispatched functions to catch the majority of errors herein.
#[allow(clippy::cognitive_complexity)] // clap 3 will have typed subcmds..
fn dispatch_commands(args: &ArgMatches) -> Result<()> {
    // listers first
    if let Some(_a) = args.subcommand_matches("list-regions") {
        let rawconf = Config::read()?;
        return shipcat::list::regions(&rawconf);
    }
    else if args.subcommand_matches("list-locations").is_some() {
        let rawconf = Config::read()?;
        return shipcat::list::locations(&rawconf);
    }
    else if let Some(a) = args.subcommand_matches("list-services") {
        let (conf , region) = resolve_config(a, ConfigType::Base)?;
        return shipcat::list::services(&conf, &region);
    }
    //if let Some(a) = args.subcommand_matches("list-products") {
    //    let l = a.value_of("location").unwrap().into();
    //    return shipcat::list::products(&conf, l);
    //}
    else if let Some(a) = args.subcommand_matches("login") {
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        return shipcat::auth::login(&conf, &region, a.is_present("force"));
    }
    // getters
    else if let Some(a) = args.subcommand_matches("get") {
        if let Some(_) = a.subcommand_matches("resources") {
            if a.is_present("region") {
                let (conf, region) = resolve_config(a, ConfigType::Base)?;
                return shipcat::get::resources(&conf, &region);
            } else {
                let rawconf = Config::read()?;
                return shipcat::get::totalresources(&rawconf);
            }
        }
        if let Some(_) = a.subcommand_matches("clusterinfo") {
            let rawconf = Config::read()?;
            assert!(a.is_present("region"), "explicit context needed for clusterinfo");
            return shipcat::get::clusterinfo(&rawconf,
                a.value_of("region").unwrap(),
                a.value_of("cluster")
            ).map(void);
        }

        // resolve region from kube context here if unspecified
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        if let Some(_) = a.subcommand_matches("versions") {
            return shipcat::get::versions(&conf, &region).map(void);
        }
        if let Some(_) = a.subcommand_matches("vault-url") {
            return shipcat::get::vault_url(&region).map(void);
        }
        if let Some(_) = a.subcommand_matches("images") {
            return shipcat::get::images(&conf, &region).map(void);
        }
        if let Some(_) = a.subcommand_matches("codeowners") {
            return shipcat::get::codeowners(&conf).map(void);
        }
        if let Some(b) = a.subcommand_matches("vault-policy") {
            let team = b.value_of("team").unwrap(); // required param
            return shipcat::get::vaultpolicy(&conf, &region, team).map(void);
        }
        if let Some(_) = a.subcommand_matches("apistatus") {
            return shipcat::get::apistatus(&conf, &region);
        }
    }
    // product
    else if let Some(_a) = args.subcommand_matches("product") {
        // TODO: handle more like the other commands
        unimplemented!();
/*        if let Some(b) = a.subcommand_matches("verify") {
            let location = b.value_of("location");
            let products  = b.values_of("products").unwrap().map(String::from).collect::<Vec<_>>();
            return shipcat::product::validate(products, &conf, location.map(String::from));
        }
        else if let Some(b) = a.subcommand_matches("show") {
            let product  = b.value_of("product").map(String::from);
            let location = b.value_of("location");
            return shipcat::product::show(product, &conf, location.unwrap());
        }*/
    }
    else if let Some(a) = args.subcommand_matches("config") {
        if let Some(_) = a.subcommand_matches("crd") {
            let (conf, _region) = resolve_config(a, ConfigType::Base)?;
            // this only works with a given region
            return shipcat::show::config_crd(conf);
        }
        // The others make sense without a region
        // Want to be able to verify full config when no kube context given!
        let conf = if a.is_present("region") {
            resolve_config(a, ConfigType::Base)?.0
        } else {
            Config::read()?
        };
        if let Some(_) = a.subcommand_matches("verify") {
            return shipcat::validate::config(conf);
        } else if let Some(_) = a.subcommand_matches("show") {
            return shipcat::show::config(conf);
        }
        unimplemented!();
    }
    // helpers that can work without a kube region, but will shell out to kubectl if not passed
    // TODO: remove this
    else if let Some(a) = args.subcommand_matches("secret") {
        let rawconf = Config::read()?;
        if let Some(b) = a.subcommand_matches("verify-region") {
            let regions = b.values_of("regions").unwrap().map(String::from).collect();
            // NB: this does a cheap verify of both Config and Manifest (vault list)
            return if b.is_present("git") {
                shipcat::validate::secret_presence_git(&rawconf, regions)
            } else if let Some(svcs) = b.value_of("services") {
                let svcvec = svcs.split(',').filter(|s| !s.is_empty()).map(String::from).collect();
                shipcat::validate::secret_presence_explicit(svcvec, &rawconf, regions)
            } else {
                shipcat::validate::secret_presence_full(&rawconf, regions)
            };
        }
    }

    // ------------------------------------------------------------------------------
    // important dev commands below - they resolve kube context as a fallback
    // otherwise region can be passed in as args

    else if let Some(a) = args.subcommand_matches("status") {
        let svc = a.value_of("service").map(String::from).unwrap();
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        return shipcat::status::show(&svc, &conf, &region)
    }
    else if let Some(a) = args.subcommand_matches("graph") {
        let dot = a.is_present("dot");
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        return if let Some(svc) = a.value_of("service") {
            if a.is_present("reverse") {
                shipcat::graph::reverse(svc, &conf, &region).map(void)
            } else {
                shipcat::graph::generate(svc, &conf, &region, dot).map(void)
            }
        } else {
            shipcat::graph::full(dot, &conf, &region).map(void)
        };
    }
    else if let Some(a) = args.subcommand_matches("validate") {
        let services = a.values_of("services").unwrap().map(String::from).collect::<Vec<_>>();
        // this only needs a kube context if you don't specify it
        let ss = if a.is_present("secrets") { ConfigType::Filtered } else { ConfigType::Base };
        let (conf, region) = resolve_config(a, ss)?;
        if !a.is_present("skip-version-check") {
            conf.verify_version_pin(&region.environment)?;
        }
        return shipcat::validate::manifest(services, &conf, &region, a.is_present("secrets"));
    }
    else if let Some(a) = args.subcommand_matches("verify") {
        return if a.value_of("region").is_some() {
            let (conf, region) = resolve_config(a, ConfigType::Base)?;
            shipcat::validate::regional_manifests(&conf, &region)
        } else {
            shipcat::validate::all_manifests()
        };
    }
    else if let Some(a) = args.subcommand_matches("values") {
        let svc = a.value_of("service").map(String::from).unwrap();

        let ss = if a.is_present("secrets") { ConfigType::Filtered } else { ConfigType::Base };
        let (conf, region) = resolve_config(a, ss)?;

        let mf = if a.is_present("secrets") {
            shipcat_filebacked::load_manifest(&svc, &conf, &region)?.complete(&region)?
        } else {
            shipcat_filebacked::load_manifest(&svc, &conf, &region)?.stub(&region)?
        };
        mf.print()?;
        return Ok(());
    }
    else if let Some(a) = args.subcommand_matches("template") {
        let svc = a.value_of("service").map(String::from).unwrap();

        let ss = if a.is_present("secrets") { ConfigType::Filtered } else { ConfigType::Base };
        let (conf, region) = resolve_config(a, ss)?;
        let ver = a.value_of("tag").map(String::from);

        let mut mf = if a.is_present("secrets") {
            shipcat_filebacked::load_manifest(&svc, &conf, &region)?.complete(&region)?
        } else {
            shipcat_filebacked::load_manifest(&svc, &conf, &region)?.stub(&region)?
        };
        mf.version = mf.version.or(ver);
        if a.is_present("current") {
            let s = status::Status::new(&mf)?;
            let crd = s.get()?;
            mf.version = mf.version.or(crd.spec.version);
            mf.uid = crd.metadata.uid;
        } else {
            // ensure valid chart
            mf.uid = Some("FAKE-GUID".to_string());
            mf.version = mf.version.or(Some("latest".to_string()));
        }
        let tpl = shipcat::helm::template(&mf, None)?;
        if a.is_present("check") {
            shipcat::helm::template_check(&mf, &region, &tpl)?;
        } else {
            println!("{}", tpl);
        }
        return Ok(());
    }
    else if let Some(a) = args.subcommand_matches("crd") {
        let svc = a.value_of("service").map(String::from).unwrap();

        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        return shipcat::show::manifest_crd(&svc, &conf, &region);
    }

    else if let Some(a) = args.subcommand_matches("env") {
        let svc = a.value_of("service").map(String::from).unwrap();
        let (conf, region) = resolve_config(a, ConfigType::Filtered)?;
        let mock = !a.is_present("secrets");
        return shipcat::env::print_bash(&svc, &conf, &region, mock);
    }

    else if let Some(a) = args.subcommand_matches("diff") {
        let svc = a.value_of("service").map(String::from).unwrap();
        let diff_exit = if a.is_present("crd") {
            // NB: no secrets in CRD
            let (conf, region) = resolve_config(a, ConfigType::Base)?;
            if a.is_present("git") {
                shipcat::diff::values_vs_git(&svc, &conf, &region)?
            } else {
                shipcat::diff::values_vs_kubectl(&svc, &conf, &region)?
            }
        } else if a.is_present("git") {
            // special - serial git diff
            // does not support mocking (but also has no secrets)
            let (conf, region) = resolve_config(a, ConfigType::Base)?;
            shipcat::diff::template_vs_git(&svc, &conf, &region)?
        } else if a.is_present("with-region") {
            // special - diff between two regions
            // does not support mocking (but also has no secrets)
            let (conf, region) = resolve_config(a, ConfigType::Base)?;
            let with_region = a.value_of("with-region").unwrap();
            let (_ref_conf, ref_region) = Config::new(ConfigType::Base, with_region)?;
            shipcat::diff::values_vs_region(&svc, &conf, &region, &ref_region)?
        } else {
            let ss = if a.is_present("secrets") { ConfigType::Filtered } else { ConfigType::Base };
            let (conf, region) = resolve_config(a, ss)?;
            let mut mf = if !a.is_present("secrets") {
                shipcat_filebacked::load_manifest(&svc, &conf, &region)?.stub(&region)?
            } else {
                shipcat_filebacked::load_manifest(&svc, &conf, &region)?.complete(&region)?
            };
            let ver = a.value_of("tag").map(String::from);
            mf.version = mf.version.or(ver);
            if a.is_present("current") {
                let s = status::Status::new(&mf)?;
                let crd = s.get()?;
                mf.version = mf.version.or(crd.spec.version);
                mf.uid = crd.metadata.uid;
            } else {
                // ensure valid chart
                mf.uid = Some("FAKE-GUID".to_string());
                mf.version = mf.version.or(Some("latest".to_string()));
            }
            let diff = if a.is_present("helm") {
                shipcat::diff::helm_diff(&mf)?
            } else {
                shipcat::diff::template_vs_kubectl(&mf)?
            };
            if let Some(mut out) = diff {
                if a.is_present("obfuscate") {
                    out = shipcat::diff::obfuscate_secrets(out, mf.get_secrets())
                };
                if a.is_present("minify") {
                    out = shipcat::diff::minify(&out)
                };
                println!("{}", out);
                false
            } else { true }
        };
        process::exit(if diff_exit { 0 } else { 1 });
    }


    else if let Some(a) = args.subcommand_matches("kong") {
        return if let Some(_b) = a.subcommand_matches("config-url") {
            let (_conf, region) = resolve_config(a, ConfigType::Base)?;
            shipcat::kong::config_url(&region)
        } else {
            let (conf, region) = resolve_config(a, ConfigType::Filtered)?;
            let mode = if a.is_present("crd") {
                kong::KongOutputMode::Crd
            } else {
                kong::KongOutputMode::Kongfig
            };
            assert!(conf.has_secrets()); // sanity on cluster disruptive commands
            shipcat::kong::output(&conf, &region, mode)
        };
    }

    else if let Some(a) = args.subcommand_matches("statuscake") {
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        return shipcat::statuscake::output(&conf, &region);
    }

    // ------------------------------------------------------------------------------
    // everything below needs a kube context!

    else if let Some(a) = args.subcommand_matches("apply") {
        let svc = a.value_of("service").map(String::from).unwrap();
        // this absolutely needs secrets..
        let (conf, region) = resolve_config(a, ConfigType::Filtered)?;
        let wait = !a.is_present("no-wait");
        let force = a.is_present("force");
        let ver = a.value_of("tag").map(String::from); // needed for some subcommands
        assert!(conf.has_secrets()); // sanity on cluster disruptive commands
        return shipcat::apply::apply(&svc, force, &region, &conf, wait, ver).map(void);
    }

    else if let Some(a) = args.subcommand_matches("restart") {
        let svc = a.value_of("service").map(String::from).unwrap();
        let (conf, region) = resolve_config(a, ConfigType::Base)?;
        let mf = shipcat_filebacked::load_manifest(&svc, &conf, &region)?;
        let wait = !a.is_present("no-wait");
        return shipcat::apply::restart(&mf, wait).map(void);
    }

    // 4. cluster level commands
    else if let Some(a) = args.subcommand_matches("cluster") {
        if let Some(b) = a.subcommand_matches("crd") {
            // This reconcile is special. It needs two config types:
            // - Base (without secrets) for putting config crd in cluster
            // - Filtered (with secrets) for actually upgrading when crds changed
            let (conf_sec, _region_sec) = resolve_config(args, ConfigType::Filtered)?;
            let (conf_base, region_base) = resolve_config(args, ConfigType::Base)?;
            let jobs = b.value_of("num-jobs").unwrap_or("8").parse().unwrap();
            if let Some(_) = b.subcommand_matches("reconcile") {
                return shipcat::cluster::mass_crd(&conf_sec, &conf_base, &region_base, jobs);
            }
        }
        if let Some(_b) = a.subcommand_matches("diff") {
            let (conf, region) = resolve_config(args, ConfigType::Filtered)?;
            return shipcat::cluster::mass_diff(&conf, &region);
        }
        if let Some(b) = a.subcommand_matches("vault-policy") {
            let (conf, region) = resolve_config(args, ConfigType::Base)?;
            let jobs = b.value_of("num-jobs").unwrap_or("8").parse().unwrap();
            if let Some(_) = b.subcommand_matches("reconcile") {
                return shipcat::cluster::mass_vault(&conf, &region, jobs);
            }
        }
    }


    // ------------------------------------------------------------------------------
    // Dispatch small helpers that does not need secrets
    // most of these require a resolved `region` via kubectl

    // super kube specific ones:
    else if let Some(a) = args.subcommand_matches("shell") {
         let (conf, region) = resolve_config(args, ConfigType::Base)?;
        let service = a.value_of("service").unwrap();
        let pod = value_t!(a.value_of("pod"), usize).ok();

        let cmd = if a.is_present("cmd") {
            Some(a.values_of("cmd").unwrap().collect::<Vec<_>>())
        } else {
            None
        };
        let mf = shipcat_filebacked::load_manifest(service, &conf, &region)?.stub(&region)?;
        return shipcat::kubectl::shell(&mf, pod, cmd);
    }
    else if let Some(a) = args.subcommand_matches("version") {
        let svc = a.value_of("service").map(String::from).unwrap();
        let (_conf, region) = resolve_config(a, ConfigType::Base)?;
        let res = shipcat::kubectl::get_running_version(&svc, &region.namespace)?;
        println!("{}", res);
        return Ok(())
    }
    else if let Some(a) = args.subcommand_matches("port-forward") {
        let (conf, region) = resolve_config(args, ConfigType::Base)?;
        let service = a.value_of("service").unwrap();
        let mf = shipcat_filebacked::load_manifest(service, &conf, &region)?.stub(&region)?;
        return shipcat::kubectl::port_forward(&mf);
    }
    else if let Some(a) = args.subcommand_matches("debug") {
        let (conf, region) = resolve_config(args, ConfigType::Base)?;
        let service = a.value_of("service").unwrap();
        let mf = shipcat_filebacked::load_manifest(service, &conf, &region)?.stub(&region)?;
        return shipcat::kubectl::debug(&mf);
    }

    // these could technically forgo the kube dependency..
    else if let Some(a) = args.subcommand_matches("slack") {
        let text = a.values_of("message").unwrap().collect::<Vec<_>>().join(" ");
        let link = a.value_of("url").map(String::from);
        let color = a.value_of("color").map(String::from);
        let msg = shipcat::slack::DumbMessage { text, link, color };
        return shipcat::slack::send_dumb(msg);
    }
    else if let Some(a) = args.subcommand_matches("gdpr") {
        let (conf, region) = resolve_config(args, ConfigType::Base)?;
        let svc = a.value_of("service").map(String::from);
        return shipcat::gdpr::show(svc, &conf, &region);
    }

    unreachable!("Subcommand valid, but not implemented");
}
