<script lang="ts">
  import LegalPage from '#lib/components/LegalPage.svelte';
</script>

<svelte:head>
  <title>Privacy policy — OvalBot</title>
</svelte:head>

<LegalPage title="Privacy policy" updated="October 2026">
  <p>
    This policy explains what personal data OvalBot processes and what your rights are. Under the
    EU General Data Protection Regulation (GDPR), you are the beneficiary of these rights and
    OvalBot's editor is the controller of the processing described below.
  </p>

  <h2>Data controller and contact</h2>
  <p>
    Controller: Julien Fichet (see the
    <a href="/legal-notice">legal notice</a>).
    <br />
    For any question or to exercise your rights:
    julien.fichet17@proton.me
  </p>

  <h2>Data we collect</h2>
  <ul>
    <li><strong>Account data</strong>: email address and a password, which is hashed with Argon2id —
      we never store it in plain text.</li>
    <li><strong>Session</strong>: a random session token stored in a browser cookie, used only to
      keep you signed in.</li>
    <li><strong>Telegram linking</strong>: if you connect a Telegram account, we store your Telegram
      chat ID so alerts can be delivered to you. It is never exposed in the interface.</li>
    <li><strong>Notification preferences</strong>: whether you want alerts by email and/or by
      Telegram.</li>
    <li><strong>Ticket requests</strong>: the criteria you declare (match, single/duo/group
      subscription, quantity, maximum price) for the alerts you asked for.</li>
    <li><strong>Waitlist status</strong>: access may be limited by an activation queue, so we store
      your account status.</li>
  </ul>
  <p>
    We do not collect bank or payment data (we never process payments), no tracking or analytics
    data, and no advertising identifiers.
  </p>

  <h2>Why we process it</h2>
  <ul>
    <li>To operate your account and provide the alerting service you asked for (performance of the
      service contract).</li>
    <li>To send you notifications you explicitly requested, on the channels and with the preferences
      you chose (consent — you can mute either channel at any time).</li>
    <li>To keep the service secure: authentication sessions only.</li>
  </ul>

  <h2>How long we keep it</h2>
  <ul>
    <li>Account and preferences: until you or an administrator delete your account.</li>
    <li>Sessions: until you log out or the session expires (30 days).</li>
    <li>Telegram linking: until you unlink the account or delete your account.</li>
    <li>Ticket requests: until you remove them or delete your account.</li>
  </ul>

  <h2>Who we share it with</h2>
  <ul>
    <li><strong>Resend</strong>, our email provider, receives your email address solely to deliver
      the emails you requested.</li>
    <li><strong>Telegram</strong> receives alert messages (your contact data stays with us) through
      its official Bot API.</li>
  </ul>
  <p>We do not sell, rent, or share your data with anyone else.</p>

  <h2>Cookies</h2>
  <p>
    The site uses a single cookie: your session token. It is strictly necessary for the site to
    work (keeping you signed in), so no consent banner is required. No tracking cookies are set.
  </p>

  <h2>Your rights</h2>
  <p>
    You can request access, rectification, erasure, restriction, data portability, or object to the
    processing of your data by writing to julien.fichet17@proton.me. Deleting your account (by an administrator, upon your request) removes your data, including your
    notifications and ticket requests. You may also lodge a complaint with the French supervisory
    authority, the <a href="https://www.cnil.fr" rel="noopener noreferrer">CNIL</a>.
  </p>

  <h2>Security</h2>
  <p>
    Passwords are hashed with Argon2id, sessions use randomly generated tokens, and personal data is
    stored on a private server. Traffic to the site should be encrypted in transit (HTTPS).
  </p>
</LegalPage>
