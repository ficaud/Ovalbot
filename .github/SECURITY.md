# Security Policy

## Reporting a Vulnerability

OvalBot handles account credentials (passwords hashed with Argon2id),
session tokens (HttpOnly cookies) and third-party secrets (a Resend API key,
a Telegram bot token and its webhook secret). Security reports are taken
seriously.

If you believe you have found a security vulnerability, please **do not open a
public issue**. Instead, report it privately using GitHub's
**Private vulnerability reporting**:

1. Go to the **Security** tab of the repository.
2. Click **Report a vulnerability**.
3. Describe the issue, the affected version, and (if possible) how to reproduce it.

You can expect:

- An acknowledgement within a few days.
- An initial assessment shortly after.
- A fix coordinated with the disclosure of the vulnerability.

## Supported Versions

OvalBot is self-hosted and pre-1.0. Only the most recent release is supported.

| Version | Supported          |
|---------|--------------------|
| latest  | :white_check_mark: |
| < 0.2.0 | :x:                |

## What to include in a report

- A clear description of the vulnerability.
- The OvalBot version (or commit SHA) and how it is deployed (Docker Compose tag).
- Steps to reproduce, if available.
- The potential impact (e.g. authentication bypass, session or secret disclosure,
  unauthorized notifications).

## Scope

In scope: authentication and session handling, the HTTP API, the Telegram
webhook verification, secret handling, and anything that could lead to account
takeover, secret disclosure or unauthorized access.

Out of scope: vulnerabilities in third-party dependencies (report them upstream),
misconfiguration of your own deployment (weak admin password, leaked `.env`,
exposed port), and physical or host-level compromise of the machine running the bot.

## Safe Harbor

We consider security research conducted in good faith — acting in the interest
of the project and its users — to be authorized. We will not pursue or support
legal action against researchers who:

- make a good-faith effort to avoid privacy violations, data destruction or
  service disruption;
- report the vulnerability privately and give us a reasonable time to fix it
  before any public disclosure;
- do not exploit the vulnerability beyond what is necessary to demonstrate it.

## Fix Timeline

We aim to:

- acknowledge your report within a few days;
- provide an initial assessment within about a week;
- release a fix within **30 days** for critical and high-severity issues, and in
  the next regular release for lower-severity ones.

Coordinated disclosure is preferred: we are happy to credit you in the release
notes unless you ask to remain anonymous.
