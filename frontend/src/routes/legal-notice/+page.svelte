<script lang="ts">
  import LegalPage from '#lib/components/LegalPage.svelte';
</script>

<svelte:head>
  <title>Legal notice — OvalBot</title>
</svelte:head>

<LegalPage title="Legal notice">
  <div class="alert alert-info">
    Draft for publication: replace every <mark class="placeholder">highlighted placeholder</mark>
    with the real details, then delete this note.
  </div>

  <p>
    OvalBot is a free, personal, non-commercial alerting service that monitors the official Stade
    Rochelais ticketing pages and notifies its users of new ticket offers, including resale offers.
    It is published by a private individual.
  </p>

  <h2>Site editor</h2>
  <dl>
    <dt>Publisher</dt>
    <dd><mark class="placeholder">[Full name]</mark> (private individual)</dd>
    <dt>Email</dt>
    <dd><mark class="placeholder">[contact@example.com]</mark></dd>
    <dt>Postal address</dt>
    <dd><mark class="placeholder">[Street, postcode, city, country]</mark></dd>
    <dt>Phone number</dt>
    <dd><mark class="placeholder">[Phone number, or remove this line]</mark></dd>
    <dt>Publication director</dt>
    <dd><mark class="placeholder">[Full name]</mark> — as the publisher of this personal site.</dd>
  </dl>

  <h2>Hosting provider</h2>
  <dl>
    <dt>Provider</dt>
    <dd><mark class="placeholder">[Hosting provider name or ISP]</mark></dd>
    <dt>Address</dt>
    <dd><mark class="placeholder">[Host's business address]</mark></dd>
    <dt>Phone number</dt>
    <dd><mark class="placeholder">[Host's phone number]</mark></dd>
  </dl>

  <h2>Nature of the service</h2>
  <ul>
    <li>
      OvalBot is an independent tool. It is <strong>not affiliated with, endorsed by</strong>, or
      operated on behalf of Stade Rochelais (Stade Rochelais SA) or any ticketing platform.
    </li>
    <li>
      OvalBot does not buy, sell, or resell tickets. It only sends notifications and links; every
      purchase is made by the user directly on the club's official ticketing website.
    </li>
    <li>No payment is processed by this site and no fee is charged for the service.</li>
  </ul>

  <p>
    See also the <a href="/privacy">privacy policy</a> and the <a href="/terms">terms of use</a>.
  </p>
</LegalPage>
