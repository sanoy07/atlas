# VestaScan Notifier — Engineering Findings

## Architecture (OBSERVED)

Standalone notification microservice. Purpose: receives events (via PubSub HTTP push), dispatches templated emails.

## Email Provider (OBSERVED)

Dual-provider: Nodemailer (SMTP) + SendGrid.
infrastructure/email/factory.ts → integration.service.ts uses EmailProviderFactory.getActiveProviderName.
template.service.ts CALLS_INSTANCE both nodemailer.adapter.ts AND sendgrid.adapter.ts — runtime provider selection.

OBSERVED: email/builder.ts + templates/email/*.html — email rendering is LOCAL (HTML templates, not third-party rendering service).
docs/email-templates.md surfaced as documentation artifact.

## Event Taxonomy (OBSERVED from prior session)

notify.handler.ts dispatches 29 CALLS_INSTANCE to notification.service.ts:
- User lifecycle: register, email verify, suspend, restore
- KYC: approved, rejected
- Token: verification approved/rejected, deployed, whitelisted, burned, revoked, minted, flagged
- Data room: files granted, access revoked
- Support: ticket created, reply, resolved
- Billing: plan changed, checkout, invoice paid, payment failed, subscription canceled/expiring, refund
- Other: interest shown

UNEXPECTED: NotificationMessage model (delivery log) is NOT imported by notification.service.ts — it's used only by external tooling (send-pubsub-http.ts test script). The notification service doesn't log its own deliveries to this model.

## Atlas Failures

1. Cross-repo PubSub topic link: notifier receives PubSub events but Atlas cannot show which vestascan-api publisher.ts topic names match which handlers. UNKNOWN.
2. Integration.service.ts purpose: calls EmailProviderFactory.getActiveProviderName but its own role (configuration vs. runtime) is UNKNOWN without source read.
