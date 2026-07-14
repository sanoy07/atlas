# VestaScan Notifier — Benchmarks

## Benchmark 8: Email Provider Architecture

Question: How does vestascan-notifier send emails and manage providers?
Commands: `atlas investigate email template provider`
Manual source reads: 0
Wrong branches: templates/email/*.html surfaced as candidates (correct — they are part of the email system)
False positives: none
Hidden understanding revealed: CALLS_INSTANCE edges revealed runtime provider dispatch: template.service.ts calls both nodemailer AND sendgrid adapters — provider is selected at runtime, not at import time. This is the first time CALLS_INSTANCE proved more valuable than CALLS_STATIC in an investigation.
Classification: Optimal
Source reads needed: 0
New primitive earned: N
Unexpected discoveries: email/builder.ts + templates/email/*.html — email rendering is entirely local (HTML template engine), not a hosted service like SendGrid's template engine. The service uses SendGrid only for SMTP, not for template rendering.

---

## Benchmark 9: Notification Event Taxonomy (from prior session)

Question: What events does vestascan-notifier handle and how is the dispatch structured?
Commands: `atlas investigate notification pubsub subscription`
Manual source reads: 1 (notification-message.model.ts — confirmed isolation was correct)
Wrong branches: none
False positives: none
Noise removed: N/A
Hidden understanding revealed: 29 CALLS_INSTANCE from notify.handler.ts with full method names — complete event taxonomy surfaced automatically
Classification: Optimal
Source reads needed: 0 (1 confirmation read)
New primitive earned: N
Unexpected discoveries: NotificationMessage model not imported by notification.service.ts — delivery log exists but service doesn't write to it directly (likely written by a separate audit mechanism or PubSub ack handler)
