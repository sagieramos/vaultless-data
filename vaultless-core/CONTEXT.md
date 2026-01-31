| Question                 | Table                    |
| ------------------------ | ------------------------ |
| Who owns the app?        | users                    |
| What is the app?         | applications             |
| Who is using it?         | clients                  |
| What plan exists?        | pricing_plans            |
| What was pricing *then*? | pricing_snapshots        |
| What happened?           | client_billing_usage     |
| What can they still do?  | client_usage_credits     |
| Why did balance change?  | credit_transactions      |
| How much was earned?     | developer_revenue_shares |
| When do we close books?  | billing_periods          |
| How do we pay?           | psp_accounts             |
| Did PSP move money?      | psp_payouts              |
| What exactly was paid?   | psp_payout_items         |


1️⃣ application_pricing_plans

Why this table exists

This table answers one very specific question:

“Which pricing plans are available for this application?”

Why it must exist:

A developer can define many pricing plans

An application should expose only some of them

One of those plans may be default

Plans can change without rewriting client subscriptions

This avoids:

Copying pricing plans per application

Hard-coding pricing into the app

Breaking existing subscribers when plans change

What role it plays in the system

It is the contract boundary between product and pricing

It is what clients actually subscribe to (indirectly)

Think of it as:

“Application catalog → available offers”

Without this table, pricing becomes brittle.

2️⃣ client_subscriptions

Why this table exists

This answers:

“Is this client on a subscription plan, and if so, which one?”

This is separate from credits on purpose.

Why subscriptions are not credits

Subscriptions define entitlement

Credits define consumption capacity

A client may:

Be subscribed and use credits

Be subscribed and consume zero usage

Have credits without a subscription (pure PAYG)

This table allows:

Free tiers

Monthly bundles

Hybrid models (subscription + overage via credits)

What it connects
client → application → pricing_plan (via application_pricing_plans)


This is business logic, not accounting.

3️⃣ developer_subscriptions

Why this table exists

This table is not about clients at all.

It answers:

“What limits and entitlements does a developer have on the platform?”

Examples:

Max applications

Max monthly messages

Feature access (webhooks, IoT, E2EE tiers)

Platform billing for developers themselves

Why this is separate from client pricing

Because:

Developers are customers of your platform

Clients are customers of developers

Conflating these is a classic SaaS mistake. You didn’t make it.

This table ensures:

Platform monetization is independent

You can charge developers even if their clients churn

Abuse prevention (rate limits, quotas)

4️⃣ client_invoices

Why this table exists

This one is often misunderstood, so let’s be precise.

client_invoices answers:

“What did we tell the client they owe or consumed for a period?”

It is:

A summary document

A human-readable artifact

A legal / audit reference

It is not:

A wallet

A payment trigger

A PSP integration point

What it summarizes

An invoice can aggregate:

Usage (client_billing_usage)

Pricing (pricing_snapshots)

Subscription fees (client_subscriptions)

Credit conversions (optional)

Why it still matters in a prepaid system

Even when clients pay upfront:

They still want statements

Developers want transparency

Audits require reconstruction

Invoices are about communication, not money movement.

5️⃣ How these 4 fit into the bigger flow

Here’s the mental map:

Pricing exposure

pricing_plans → defines prices

application_pricing_plans → exposes prices per app

Client commitment

client_subscriptions → client chooses a plan

Usage & consumption

client_billing_usage → tracks what happened

client_usage_credits → enforces limits

Accounting & reporting

pricing_snapshots → locks prices

client_invoices → explains charges

developer_revenue_shares → attributes earnings

Platform monetization

developer_subscriptions → bills developers

psp_* → moves real money

| Table                     | Prevents                              |
| ------------------------- | ------------------------------------- |
| application_pricing_plans | Breaking clients when pricing changes |
| client_subscriptions      | Mixing entitlement with consumption   |
| developer_subscriptions   | Platform revenue leakage              |
| client_invoices           | Billing disputes & audit failures     |

📄 How Billing Works (One-Page Overview)
1. Core idea

The platform separates money, credits, usage, and entitlement.

Money is handled by PSPs (Paystack, Stripe, etc.)

Credits are platform-internal, non-cash units

Usage is metered per application

Entitlement is defined by subscriptions and pricing plans

This separation allows:

Multi-currency support

Cross-developer credit usage

Auditable revenue sharing

PSP-agnostic payouts

2. Buying credits (cash → credits)

A client pays real money via a PSP (e.g. Paystack, currency = NGN).

The platform:

Confirms payment

Converts cash → platform credits using a locked FX rate

Records a credit transaction

Credits are added to the client’s global credit balance.

Important rule
Credits are not money and do not belong to any developer yet.

They are platform-held value.

3. Pricing & subscriptions (what usage costs)

Developers define pricing plans (currency-denominated).

Applications expose selected pricing plans.

Clients may subscribe to a plan or pay purely via credits (PAYG).

At the moment usage starts, the platform:

Captures a pricing snapshot

Freezes price, currency, and conversion logic

This ensures pricing consistency even if plans change later.

4. Usage & consumption (credits → revenue)

A client uses an application.

Usage is metered (messages, bandwidth, proofs, etc.).

Usage is converted into credit cost using the pricing snapshot.

Credits are deducted from the client balance.

That consumed credit is attributed to the developer as earned revenue.

At this point:

The developer becomes entitled to revenue

The platform takes its commission

The PSP still holds the cash

5. Invoicing & reporting (clarity layer)

Client invoices summarize:

Usage

Pricing

Credits consumed

Developer revenue reports summarize:

Gross usage value

Platform fees

Net payable amount

Invoices are descriptive, not payment triggers.

6. Payouts (platform → developer)

Developers accumulate earned revenue.

Every payout cycle (e.g. 30 days):

Eligible balances are aggregated

A payout instruction is created

The platform tells the PSP:

Who to pay

How much

In which currency

PSP executes the payout.

The platform never guesses — it only pays earned usage.

7. Why this model works

Credits float across developers

FX risk is controlled

Pricing changes don’t rewrite history

Money movement is isolated and auditable

New PSPs can be added without touching billing logic

🧪 Stress-Testing Edge Cases

Now let’s break it and see what survives.

1️⃣ Refunds (partial or full)
Scenario

Client buys credits, then requests a refund.

What happens

Only unused credits are refundable.

Refund amount is calculated from:

Original purchase FX rate

Remaining credit balance

Used credits are never refundable (they already became developer revenue).

Why your system survives

Credits consumed are already attributed

Pricing snapshots preserve historical value

No need to claw back developer earnings

👉 This avoids retroactive chaos.

2️⃣ FX swings (NGN ↔ USD volatility)
Scenario

Client buys credits in NGN.
Developer prices in USD.
FX rate changes dramatically later.

What happens

FX conversion is locked at credit purchase

Usage consumes credits at fixed internal value

Developer earnings are based on usage snapshots, not live FX

Who carries FX risk?

The platform (by design)

Not the developer

Not retroactively the client

Why this is correct

Predictable pricing

No arbitrage between developers

Clean accounting

You can later introduce:

FX buffers

Dynamic credit pricing

Multi-currency credit pools

But v1 stays stable.

3️⃣ Plan changes mid-period
Scenario

Developer changes pricing while clients are active.

What happens

Existing usage continues under old pricing snapshots

New usage uses the new snapshot

No re-pricing of past usage

Why this is essential

Without snapshots:

Invoices change after the fact

Developers dispute payouts

Clients lose trust

Your design explicitly prevents this.

4️⃣ Client runs out of credits mid-usage
Options (policy decision)

Hard stop usage

Grace usage → negative balance

Auto-top-up

Why your schema supports all three

Because:

Credits are centralized

Usage is metered independently

No money is assumed in real-time

Policy can evolve without schema rewrite.

5️⃣ Developer switches payout currency
Scenario

Developer wants USD instead of NGN.

What happens

Revenue remains recorded in normalized internal units

Payout conversion happens only at payout time

PSP handles FX where possible

Why this is clean

No historical mutation

No re-pricing usage

PSP-agnostic design remains intact

Final confidence check

Your system:

Does not lie about money

Does not rewrite history

Does not entangle PSP logic with billing

Scales from Nigeria → global