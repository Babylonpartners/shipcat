use slack_hook::{Slack, PayloadBuilder, SlackLink, SlackText, SlackUserLink, AttachmentBuilder};
use slack_hook::SlackTextContent::{self, Text, Link, User};
use std::env;
use semver::Version;

use shipcat_definitions::structs::{Contact, Metadata};
use shipcat_definitions::config::NotificationMode;
use shipcat_definitions::region::{Environment};
use crate::diff;
use super::{Config, Result, ErrorKind, ResultExt};

/// Slack message options we support
///
/// These parameters get distilled into the attachments API.
/// Mostly because this is the only thing API that supports colour.
#[derive(Debug, Clone)]
pub struct Message {
    /// Text in message
    pub text: String,

    /// Metadata from Manifest
    pub metadata: Metadata,

    /// Optional color for the attachment API
    pub color: Option<String>,

    /// Optional code input
    pub code: Option<String>,

    /// Optional version to send when not having code diffs
    pub version: Option<String>,
}

#[derive(Debug, Clone, Default)]
pub struct DumbMessage {
    /// Text in message
    pub text: String,

    /// Replacement link for CI infer
    pub link: Option<String>,

    /// Optional color for the attachment API
    pub color: Option<String>,
}


pub fn env_hook_url() -> Result<String> {
    env::var("SLACK_SHIPCAT_HOOK_URL").map_err(|_| ErrorKind::MissingSlackUrl.into())
}
pub fn env_channel() -> Result<String> {
    env::var("SLACK_SHIPCAT_CHANNEL").map_err(|_| ErrorKind::MissingSlackChannel.into())
}
fn env_username() -> String {
    env::var("SLACK_SHIPCAT_NAME").unwrap_or_else(|_| "shipcat".into())
}

/// Basic check to see that slack credentials is working
///
/// Used before running upgrades so we have a trail
/// It's not very good at the moment. TODO: verify better
pub fn have_credentials() -> Result<()> {
    env_channel()?;
    env_hook_url()?;
    Ok(())
}

/// Send a message based on a upgrade event
pub fn send(msg: Message, conf: &Config, env: &Environment) -> Result<()> {
    let hook_chan : String = env_channel()?;
    send_internal(msg.clone(), hook_chan, &conf, &env)?;
    let md = &msg.metadata;
    if let Some(chan) = &md.notifications {
        let c = chan.clone();
        send_internal(msg, c.to_string(), &conf, &env)?;
    }
    Ok(())
}

/// Send entry point for `shipcat slack`
pub fn send_dumb(msg: DumbMessage) -> Result<()> {
    let chan : String = env_channel()?;
    let hook_url : &str = &env_hook_url()?;
    let hook_user : String = env_username();

    // if hook url is invalid, chain it so we know where it came from:
    let slack = Slack::new(hook_url).chain_err(|| ErrorKind::SlackSendFailure(hook_url.to_string()))?;
    let mut p = PayloadBuilder::new().channel(chan)
      .icon_emoji(":cat:")
      .username(hook_user);

    let mut a = AttachmentBuilder::new(msg.text.clone()); // <- fallback
    if let Some(c) = msg.color {
        a = a.color(c)
    }
    // All text constructed for first attachment goes in this vec:
    let mut texts = vec![Text(msg.text.into())];

    // Optional replacement link
    if let Some(link) = msg.link {
        let split: Vec<&str> = link.split('|').collect();
        // Full sanity check here as it could come from the CLI
        if split.len() > 2 {
            bail!("Link {} not in the form of url|description", link);
        }
        let desc = if split.len() == 2 { split[1].into() } else { link.clone() };
        let addr = if split.len() == 2 { split[0].into() } else { link.clone() };
        texts.push(Link(SlackLink::new(&addr, &desc)));
    } else {
        // Auto link/text from originator if no ink set
        texts.push(infer_ci_links());
    }

    // Pass the texts array to slack_hook
    a = a.text(texts.as_slice());
    let ax = vec![a.build()?];
    p = p.attachments(ax);

    // Send everything. Phew.
    slack.send(&p.build()?).chain_err(|| ErrorKind::SlackSendFailure(hook_url.to_string()))?;
    Ok(())
}

/// Send a `Message` to a configured slack destination
fn send_internal(msg: Message, chan: String, conf: &Config, env: &Environment) -> Result<()> {
    let hook_url : &str = &env_hook_url()?;
    let hook_user : String = env_username();
    let md = &msg.metadata;

    // if hook url is invalid, chain it so we know where it came from:
    let slack = Slack::new(hook_url).chain_err(|| ErrorKind::SlackSendFailure(hook_url.to_string()))?;
    let mut p = PayloadBuilder::new().channel(chan)
      .icon_emoji(":ship:")
      .username(hook_user);

    debug!("Got slack notify {:?}", msg);
    // NB: cannot use .link_names due to https://api.slack.com/changelog/2017-09-the-one-about-usernames
    // NB: cannot use .parse(Parse::Full) as this breaks the other links
    // Thus we have to use full slack names, and construct SlackLink objs manually

    // All text is in either one or two attachments to make output as clean as possible

    // First attachment is main text + main link + CCs
    // Fallbacktext is in constructor here (shown in OSD notifies)
    let mut a = AttachmentBuilder::new(msg.text.clone()); // <- fallback
    if let Some(c) = msg.color {
        a = a.color(c)
    }
    // All text constructed for first attachment goes in this vec:
    let mut texts = vec![Text(msg.text.into())];

    let mut codeattach = None;
    if let Some(diff) = msg.code {
        // does the diff contain versions?
        let is_version_only = if let Some((v1, v2)) = diff::infer_version_change(&diff) {
            let lnk = create_github_compare_url(&md, (&v1, &v2));
            texts.push(lnk);
            diff::is_version_only(&diff, (&v1, &v2))
        } else {
            false
        };
        // is diff otherwise meaningful?
        if !is_version_only {
            codeattach = Some(AttachmentBuilder::new(diff.clone())
                .color("#439FE0")
                .text(vec![Text(diff.into())].as_slice())
                .build()?)
        }
    } else if let Some(v) = msg.version {
        texts.push(infer_metadata_single_link(md, v));
    }

    // Automatic CI originator link
    texts.push(infer_ci_links());

    let notificationMode = if let Some(team) = conf.teams.iter().find(|t| t.name == md.team) {
        team.slackSettings.get(&env).unwrap_or(&NotificationMode::NotifyMaintainers)
    } else {
        &NotificationMode::NotifyMaintainers
    };

    // Auto cc users
    match notificationMode {
        NotificationMode::NotifyMaintainers => {
            texts.push(Text("<- ".to_string().into()));
            texts.extend(contacts_to_text_content(&md.contacts));
        }
        _ => {},
    }

    // Pass the texts array to slack_hook
    a = a.text(texts.as_slice());
    let mut ax = vec![a.build()?];

    // Second attachment: optional code (blue)
    if let Some(diffattach) = codeattach {
        ax.push(diffattach);
        // Pass attachment vector

    }
    p = p.attachments(ax);

    // Send everything. Phew.
    if notificationMode != &NotificationMode::Silent {
        slack.send(&p.build()?).chain_err(|| ErrorKind::SlackSendFailure(hook_url.to_string()))?;
    }

    Ok(())
}

fn short_ver(ver: &str) -> String {
    if Version::parse(&ver).is_err() && ver.len() == 40 {
        // only abbreviate versions that are not semver and 40 chars (git shas)
        ver[..8].to_string()
    } else {
        ver.to_string()
    }
}

fn infer_metadata_single_link(md: &Metadata, ver: String) -> SlackTextContent {
    let url = if Version::parse(&ver).is_ok() {
        let tag = md.version_template(&ver).unwrap_or(ver.to_string());
        format!("{}/releases/tag/{}", md.repo, tag)
    } else {
        format!("{}/commit/{}", md.repo, ver)
    };
    Link(SlackLink::new(&url, &short_ver(&ver)))
}

fn create_github_compare_url(md: &Metadata, vers: (&str, &str)) -> SlackTextContent {
    let (v0, v1) = if Version::parse(vers.0).is_ok() {
        let v0 = md.version_template(&vers.0).unwrap_or(vers.0.to_string());
        let v1 = md.version_template(&vers.1).unwrap_or(vers.1.to_string());
        (v0, v1)
    } else {
        (vers.0.into(), vers.1.into())
    };
    let url = format!("{}/compare/{}...{}", md.repo, v0, v1);
    Link(SlackLink::new(&url, &short_ver(vers.1)))
}

fn contacts_to_text_content(contacts: &Vec<Contact>) -> Vec<SlackTextContent> {
    contacts.iter().map(|cc| { User(SlackUserLink::new(&cc.slack)) }).collect()
}

/// Infer originator of a message
fn infer_ci_links() -> SlackTextContent {
    if let (Ok(url), Ok(name), Ok(nr)) = (env::var("BUILD_URL"),
                                          env::var("JOB_NAME"),
                                          env::var("BUILD_NUMBER")) {
        // we are on jenkins
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let (Ok(url), Ok(name), Ok(nr)) = (env::var("CIRCLE_BUILD_URL"),
                                                 env::var("CIRCLE_JOB"),
                                                 env::var("CIRCLE_BUILD_NUM")) {
        // we are on circle
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let Ok(user) = env::var("USER") {
        Text(SlackText::new(format!("(via {})", user)))
    } else {
        warn!("Could not infer ci links from environment");
        Text(SlackText::new("via unknown user".to_string()))
    }
}
