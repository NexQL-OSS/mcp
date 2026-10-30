---
layout: ../../layouts/DocsLayout.astro
slug: privacy
eyebrow: Guide · 03
title: Masking personal data
description: Which columns come back as <redacted>, how NexQL follows them through your SQL, and how to tune it.
---

Query results become part of your conversation, and your AI provider sees them. So before results leave nexql-mcp, columns that hold personal data or secrets are replaced with `<redacted>`. It's on by default, with nothing to set up.

## What gets masked

**Secrets**, by column name:

- names starting with `password`, or ending in `_password`
- names ending in `_hash` (`password_hash`, `api_key_hash`)
- anything containing `token`
- `secret` as a whole word or segment (`client_secret`), but not inside another word (`secretary`)
- `ssn`

**Personal data**, by column name:

- **Email:** `email`, `*_email`, `email_address`, `e_mail`
- **Phone:** `phone`, `*_phone`, `phone_number`, `mobile`, `fax`
- **Address:** `address`, `*_address` (including `ip_address`), `address_line1`, `street*`
- **A person's name:** `first_name`, `last_name`, `full_name`, `middle_name`, `surname`, `username`, and `customer_name`, `employee_name`, `patient_name` and similar
- **Date of birth:** `dob`, `date_of_birth`, `birth_date`, `birthday`
- **Identifiers:** `passport*`, `national_id`, `tax_id`, `card_number`, `iban`

Names are matched on `_`-separated parts, so flags and counts next to the data (`email_verified`, `phone_count`) are not masked, and neither are `name`, `product_name`, `city` or `postal_code`.

**Columns you list**, in a profile:

```toml
pii_columns = ["public.users.notes", "billing.cards.last4"]
```

## It follows the data, not just the name

Matching column names alone would be easy to get around. nexql-mcp parses each query and traces every result column back to the table columns it reads, so all of these come back masked:

```sql
SELECT email AS contact FROM users;
SELECT lower(email) FROM users;
WITH u AS (SELECT email FROM users) SELECT email AS contact FROM u;
SELECT email FROM user_emails;   -- a view over users.email
```

Whole-row constructs that would pack a masked column into one value, such as `row_to_json(u)`, are refused instead.

The same masking applies to [`sample_values`](/docs/tools/sample-values) and [`export_query`](/docs/tools/export-query).

## When the assistant hits a mask

The assistant is told that masked values are deliberate and to tell you, not to look for a way around it. You can still count, group and filter on masked columns. Only their values are hidden:

```sql
SELECT count(*) FROM users WHERE email LIKE '%@example.com';
```

## Tuning it

To see personal data in results, for example on a local development database full of fake data, turn the personal-data rules off for that profile. Secrets stay masked:

```toml
[profiles.local]
url = "postgres://dev@localhost:5432/appdb"
personal_data_heuristics = false
```

`sensitive_name_heuristics = false` turns off the secret rules too. Do that only for a database with no real secrets in it.

> Masking hides values in results. It doesn't stop the database from reading them, and it doesn't replace a role with the right privileges. For production data, connect as a role that can only read what the assistant needs.
