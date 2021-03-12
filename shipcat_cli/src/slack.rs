use semver::Version;
use slack_hook2::{
    AttachmentBuilder, PayloadBuilder, Slack, SlackLink, SlackText,
    SlackTextContent::{self, Link, Text, User},
    SlackUserLink,
};
use std::{collections::BTreeMap, env};

use super::{ErrorKind, Result};
use crate::diff;
use shipcat_definitions::{
    structs::{Contact, Metadata, NotificationMode},
    teams::{Owners, Person},
};

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

    /// Notification Mode from Manifest
    pub mode: NotificationMode,

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
pub async fn send(msg: Message, owners: &Owners) -> Result<()> {
    let hook_chan: String = env_channel()?;
    send_internal(msg.clone(), hook_chan, owners).await?;
    let md = &msg.metadata;
    if let Some(chan) = &md.notifications {
        let c = chan.clone();
        send_internal(msg, c.to_string(), owners).await?;
    }
    Ok(())
}

/// Send entry point for `shipcat slack`
pub async fn send_dumb(msg: DumbMessage) -> Result<()> {
    let chan: String = env_channel()?;
    let hook_url: &str = &env_hook_url()?;
    let hook_user: String = env_username();

    let slack = Slack::new(hook_url)?;
    let mut p = PayloadBuilder::new()
        .channel(chan)
        .icon_emoji(":shipcat:")
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
        let desc = if split.len() == 2 {
            split[1].into()
        } else {
            link.clone()
        };
        let addr = if split.len() == 2 {
            split[0].into()
        } else {
            link.clone()
        };
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
    slack.send(&p.build()?).await?;
    Ok(())
}

/// Send a `Message` to a configured slack destination
async fn send_internal(msg: Message, chan: String, owners: &Owners) -> Result<()> {
    let hook_url: &str = &env_hook_url()?;
    let hook_user: String = env_username();
    let md = &msg.metadata;

    let slack = Slack::new(hook_url)?;

    let mut p = PayloadBuilder::new()
        .channel(chan)
        .icon_emoji(":shipcat:")
        .username(hook_user);

    debug!("Got slack notify {:?}", msg);
    // NB: cannot use .link_names due to https://api.slack.com/changelog/2017-09-the-one-about-usernames
    // NB: cannot use .parse(Parse::Full) as this breaks the other links
    // Thus we have to use full slack names, and construct SlackLink objs manually

    // All text is in either one or two attachments to make output as clean as possible

    // First attachment is main text + main link + CCs
    // Fallbacktext is in constructor here (shown in OSD notifies)
    let mut a = AttachmentBuilder::new(msg.text.clone()); // fallback
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
            codeattach = Some(
                AttachmentBuilder::new(diff.clone())
                    .color("#439FE0")
                    .text(vec![Text(diff.into())].as_slice())
                    .build()?,
            )
        }
    } else if let Some(v) = msg.version {
        texts.push(infer_metadata_single_link(md, v));
    }

    // Automatic CI originator link
    texts.push(infer_ci_links());

    // Auto cc users
    if let NotificationMode::NotifyMaintainers = msg.mode {
        if !md.contacts.is_empty() || !md.maintainers.is_empty() {
            texts.push(Text("<- ".to_string().into()));
        }
        // maintainer strings via people in teams.yml
        if !md.maintainers.is_empty() {
            texts.extend(maintainers_to_text_content(&md.maintainers, &owners.people))
        }
        // legacy contacts:
        if !md.contacts.is_empty() {
            texts.extend(contacts_to_text_content(&md.contacts));
        }
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
    if msg.mode != NotificationMode::Silent {
        slack.send(&p.build()?).await?;
    }

    Ok(())
}

pub fn short_ver(ver: &str) -> String {
    if Version::parse(&ver).is_err() && ver.len() == 40 {
        // only abbreviate versions that are not semver and 40 chars (git shas)
        ver[..8].to_string()
    } else {
        ver.to_string()
    }
}

fn infer_metadata_single_link(md: &Metadata, ver: String) -> SlackTextContent {
    let url = md.github_link_for_version(&ver);
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
    let url = if md.repo.contains("/tree/") {
        // subfolder specified in tree - cannot do nice compare url for that
        md.repo.clone()
    } else {
        format!("{}/compare/{}...{}", md.repo, v0, v1)
    };
    Link(SlackLink::new(&url, &short_ver(vers.1)))
}

fn contacts_to_text_content(contacts: &[Contact]) -> Vec<SlackTextContent> {
    contacts
        .iter()
        .map(|cc| User(SlackUserLink::new(&cc.slack)))
        .collect()
}

fn maintainers_to_text_content(
    maintainers: &[String],
    people: &BTreeMap<String, Person>,
) -> Vec<SlackTextContent> {
    maintainers
        .iter()
        .filter_map(|m| people.get(m))
        .map(|p| User(SlackUserLink::new(&format!("@{}", &p.slack))))
        .collect()
}

/// Infer originator of a message
fn infer_ci_links() -> SlackTextContent {
    if let (Ok(url), Ok(name), Ok(nr)) = (
        env::var("BUILD_URL"),
        env::var("JOB_NAME"),
        env::var("BUILD_NUMBER"),
    ) {
        // we are on jenkins
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let (Ok(url), Ok(name), Ok(nr)) = (
        env::var("CIRCLE_BUILD_URL"),
        env::var("CIRCLE_JOB"),
        env::var("CIRCLE_BUILD_NUM"),
    ) {
        // we are on circle
        Link(SlackLink::new(&url, &format!("{}#{}", name, nr)))
    } else if let Ok(user) = env::var("USER") {
        Text(SlackText::new(format!("(via {})", user)))
    } else {
        warn!("Could not infer ci links from environment");
        Text(SlackText::new("via unknown user".to_string()))
    }
}
