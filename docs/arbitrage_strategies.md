# Polymarket Arbitrage Strategies Summary

Research compiled from Twitter/X, Reddit, and crypto trading communities (2024-2026).

---

## Table of Contents
1. [Cross-Platform Arbitrage](#1-cross-platform-arbitrage)
2. [Latency/Lag Arbitrage](#2-latencylag-arbitrage)
3. [Intra-Market Arbitrage](#3-intra-market-arbitrage)
4. [Options + Prediction Market Hedging](#4-options--prediction-market-hedging)
5. [Market Making / LP Strategies](#5-market-making--lp-strategies)
6. [Domain Expertise Strategies](#6-domain-expertise-strategies)
7. [Notable Bot/Trader Performance](#7-notable-bottrader-performance)
8. [Deep Dive: RN1's "Holy Trinity Farming"](#8-deep-dive-rn1s-holy-trinity-farming-strategy)
9. [Deep Dive: "ilovecircle" AI Strategy](#9-deep-dive-ilovecircle-ai-trading-strategy)
10. [Deep Dive: French Whale "Théo"](#10-deep-dive-french-whale-théo-strategy)
11. [Risks & Challenges](#11-risks--challenges)
12. [Tools & Resources](#12-tools--resources)
13. [Sources](#13-sources)

---

## 1. Cross-Platform Arbitrage

### Basic Principle
In prediction markets, YES + NO = $1.00 guaranteed. Arbitrage exists when:
```
Best YES ask (Platform A) + Best NO ask (Platform B) < $1.00
```

### Example
- Kalshi YES ask: $0.42
- Polymarket NO ask: $0.56
- **Total cost: $0.98**
- **Guaranteed return: $1.00**
- **Profit: $0.02 per contract (2%)**

### Platforms to Monitor
| Platform | Settlement | Regulation | Notes |
|----------|------------|------------|-------|
| Polymarket | USDC (Polygon) | Unregulated | Highest liquidity, leads price discovery |
| Kalshi | USD | CFTC-regulated | US-focused, slower but regulatory certainty |
| PredictIt | USD | CFTC no-action letter | Lower limits, higher fees |
| Opinion | Various | Varies | Mirrors many Polymarket markets |

### Reported Returns
- 12-20% monthly returns with liquidity-aware execution
- ~$4,000 profit reported from 2024 election arbitrage between Kalshi/Polymarket
- $40 million in arbitrage profits extracted from Polymarket (Apr 2024 - Apr 2025)

### Key Risk: Resolution Differences
- **Polymarket**: Uses "consensus of credible reporting", disputes go to UMA token holders
- **Kalshi**: Names specific sources (White House, NYT) as resolution authorities
- **Example**: 2024 government shutdown - Polymarket required "OPM announcement", Kalshi required "actual shutdown >24 hours"

---

## 2. Latency/Lag Arbitrage

### The "Jane Street Polymarket Lag Arb Bot" Strategy
Exploits the delay between spot price movements on CEXs (Binance, Coinbase) and Polymarket price updates.

### How It Works
1. Monitor BTC/ETH spot prices on Binance/Coinbase
2. Detect significant price movement (e.g., >0.1% in 1 second)
3. Check if Polymarket 15-min Up/Down markets haven't updated yet
4. Buy the appropriate YES/NO position before market catches up
5. Sell when Polymarket prices align with spot

### Performance Claims
- One bot: $313 → $414,000 in one month (98% win rate)
- Another bot: $25,000 profit in 9 days trading BTC/SOL/ETH/XRP
- Anonymous trader: $1,000 → $2,000,000 via microstructure arbitrage (13,000+ trades)

### Platform Response
Polymarket introduced **dynamic taker fees** to counter this:
- Fees highest at 50% odds (~3.15% on 50-cent contracts)
- Designed to exceed typical arbitrage margins
- Makes pure latency arbitrage unprofitable for most

---

## 3. Intra-Market Arbitrage

### Dutch Book Arbitrage
When sum of all outcome probabilities < 100%, guaranteed profit exists.

### Example
- Market with 3 outcomes: A, B, C
- If YES prices: A=$0.33, B=$0.33, C=$0.33 = $0.99 total
- Buy all three for $0.99, one MUST pay $1.00
- **Profit: $0.01 (1%)**

### Combinatorial/Bundle Arbitrage
Exploit inconsistencies between related markets:
- "Trump wins 2024" vs "Republicans win 2024"
- Presidential winner vs specific state outcomes
- Cabinet picks correlated with election results

### Cornell Study Finding
> "Polymarket is an arbitrage engine, not a casino."

---

## 4. Options + Prediction Market Hedging

### BitMEX Strategy Example
Combine BitMEX options with Polymarket hedge:

1. **Long Position**: BitMEX bull call spread on BTC
2. **Hedge**: "No" position on Polymarket (BTC won't hit target)

### Correlated Asset Lag
- Bet on cabinet picks immediately after presidential win confirmed
- Hedge crypto portfolio against ETF approval/denial events
- Correlate macro events with crypto prediction markets

---

## 5. Market Making / LP Strategies

### LP Farming on New Markets
- New markets launch with low liquidity
- LPs can earn 80-200% APY equivalent
- Also increases Polymarket airdrop probability

### "Nearly Resolved" Markets
- Target markets priced 95%+ with resolution imminent
- 1-5% return per trade seems small
- 5% in 24 hours = ~1,800% annualized

### Requirements
- Fast execution infrastructure
- Multiple exchange API connections
- Automated position management

---

## 6. Domain Expertise Strategies

### Key Insight
> "Top 5 on the all-time PnL leaderboard all made their money in US politics."

### Successful Approaches
- Deep research in politics, economics, current events
- Develop independent opinions ahead of crowd
- News analysis + baseline pricing + sentiment
- Informational arbitrage from domain knowledge

### Notable Trader: Domer (@ImJustKen)
- Trading prediction markets since 2007
- ~10,000 predictions
- $2.5M+ net profit
- $100K in single week (Dec 2025)

---

## 7. Notable Bot/Trader Performance

| Bot/Trader | Profit | Timeframe | Strategy |
|------------|--------|-----------|----------|
| **RN1** | $1K → $2M | 2025 | Holy Trinity Farming (see below) |
| BTC/ETH/SOL Bot | $313 → $414K | 1 month | Latency arbitrage (98% win) |
| BoshBashBish | $25K | 9 days | Up/Down market timing |
| **ilovecircle** | $2.2M | 60 days | AI probability models |
| **Théo** (French) | $85M | 2024 election | HFT + large position sizing |
| Election Arb | $4K | 2024 election | Cross-platform (Kalshi/Poly) |

---

## 8. Deep Dive: RN1's "Holy Trinity Farming" Strategy

The most documented successful Polymarket strategy comes from anonymous trader "RN1" who turned $1,000 into $2,000,000.

### Key Statistics
- **P&L**: $2M from $1K start
- **Volume**: $92M+ total
- **Trades**: 13,000+ (primarily sports markets)
- **Best single trade**: $129,000 profit
- **Core approach**: Microstructure arbitrage, NOT outcome prediction

### The Three Pillars

#### Pillar 1: Never Close Positions (Synthetic Sells)
```
Traditional approach:
  Buy "Team A wins" → Sell position → Pay taker fees

RN1's approach:
  Buy "Team A wins" → Buy "Team A loses" or "Draw"
  = Creates neutral position WITHOUT paying taker fees
```

**Why it works**: Selling in illiquid order books incurs high taker fees. By buying the opposite outcome instead, RN1 achieves the same risk-neutral position while avoiding fees.

#### Pillar 2: Trash Farming (Volume Mining)
At the end of events, RN1 buys contracts almost certain to lose at $0.01-$0.03:

```
Example:
  - Buy $10 worth of contracts at $0.01
  - Generates $1,000 in notional trading volume
  - Platform rewards (points, airdrops, rebates) > $10 loss
  - Profit comes from farming rewards, not the bet
```

#### Pillar 3: AMM Microstructure Exploitation
RN1 exploits temporary price discrepancies in Polymarket's automated market maker:

1. **Identify irrational pricing** when market moves too fast
2. **Act as rebalancing force** by providing liquidity
3. **Capture spread** when prices normalize
4. **Repeat at high frequency** (5,000+ trades)

### Key Insight
> "He doesn't try to predict the future, he exploits inefficiencies in the present."

RN1's strategy is purely mathematical—no narratives, no opinions, just execution on market inefficiencies.

### Market Focus
- Primarily **sports markets** (football, etc.)
- These have frequent events = more arbitrage opportunities
- Higher volume = more microstructure inefficiencies

---

## 9. Deep Dive: "ilovecircle" AI Trading Strategy

Another notable trader made $2.2M in 60 days using AI.

### Technical Implementation
- Used **Claude AI** to generate Python scripts
- Scripts connected to Polymarket API for:
  - Authentication
  - Pricing data retrieval
  - Trade execution
- AI helped with real-time debugging

### Core Logic
```python
# Simplified probability comparison
polymarket_implied_prob = share_price  # e.g., 0.60 = 60%
ai_model_prob = calculate_probability(live_data)  # e.g., 0.75

if ai_model_prob > polymarket_implied_prob + threshold:
    execute_buy()
```

### Performance
- **74% accuracy** across trades
- Markets: sports, crypto events, political outcomes
- Repeated logic thousands of times

---

## 10. Deep Dive: French Whale "Théo" Strategy

Made $85M on Trump's 2024 victory.

### Approach
- Controlled **11+ separate accounts**
- Wagered **$70M+** on Trump victory
- High-frequency trading: **1,600+ trades in 24 hours** during peaks

### Obfuscation Techniques
- Mixed large orders ($4,302) with small ones ($0.30-$187)
- Used multiple accounts to avoid detection
- Spread activity across time

---

## 11. Risks & Challenges

### Execution Risks
- **Liquidity crunches**: Large positions cause severe slippage
- **Binary volatility**: Single event can zero your position
- **Capital lockups**: Long-duration events = illiquid capital

### Information Risks
- **Insider risk**: Someone always has more information
- **Resolution risk**: Platforms interpret events differently

### Technical Challenges
- Opportunities exist only seconds to minutes
- Manual trading cannot compete with bots
- Infrastructure requirements: low-latency nodes, multiple APIs

### Fee Impact
- Transaction fees can eliminate small arbitrage margins
- Polymarket dynamic fees specifically target latency arbitrage
- Cross-platform fees compound

---

## 12. Tools & Resources

### Open Source
- [poly-kalshi-arb](https://github.com/taetaehoho/poly-kalshi-arb) - Rust-based cross-platform arbitrage
- [polymarket-kalshi-btc-arbitrage-bot](https://github.com/CarlosIbCu/polymarket-kalshi-btc-arbitrage-bot) - BTC 1-hour market arbitrage

### Commercial/Community
- **eventarb.com** - Cross-platform arbitrage calculator
- **Oddpool** - "Bloomberg of prediction markets" - aggregates odds across platforms
- **getarbitragebets.com** - Arbitrage opportunity scanner

### Research
- [Cornell Study on Polymarket Arbitrage](https://arxiv.org/abs/2508.03474) - "Unravelling the Probabilistic Forest"
- [SSRN: Price Discovery in Prediction Markets](https://papers.ssrn.com/sol3/papers.cfm?abstract_id=5331995)

---

## 13. Sources

### Twitter/X
- [@PixOnChain - $100K Arbitrage Guide](https://x.com/PixOnChain/status/1914377412126638095)
- [@ultra_taker - BoshBashBish Bot Analysis](https://x.com/ultra_taker/status/2000942666582728823)
- [@Cointelegraph - $1K to $2M Trader](https://x.com/Cointelegraph/status/2004598284526969166)
- [@BitMEX - Options + Polymarket Hedge](https://x.com/BitMEX/status/1889615880486789226)
- [@leviathan_news - 12-20% Monthly Returns](https://x.com/leviathan_news/status/2007769183031877664)
- [@areebkhan280 - Election Arbitrage](https://x.com/areebkhan280/status/1854281704120471624)
- [@bankrbot - Jane Street Lag Arb Strategy](https://x.com/bankrbot/status/2006112768152510856)
- [@michael_lwy - Resolution Risk Analysis](https://x.com/michael_lwy/status/1925890768654254571)
- [@hackapreneur - RN1 Microstructure Analysis](https://x.com/hackapreneur/status/2004552276674003141)
- [@qwerty_ytrevvq - RN1 Strategy Breakdown](https://x.com/qwerty_ytrevvq/status/2004248367082172562)
- [@PolymarketStory - RN1 PnL Curve](https://x.com/PolymarketStory/status/2006324635865158049)

### Articles
- [Yahoo Finance - Arbitrage Bots Dominate Polymarket](https://finance.yahoo.com/news/arbitrage-bots-dominate-polymarket-millions-100000888.html)
- [BeInCrypto - How Bots Make Millions](https://beincrypto.com/arbitrage-bots-polymarket-humans/)
- [QuantVPS - Polymarket HFT with AI](https://www.quantvps.com/blog/polymarket-hft-traders-use-ai-arbitrage-mispricing)
- [Finance Magnates - Dynamic Fees](https://www.financemagnates.com/cryptocurrency/polymarket-introduces-dynamic-fees-to-curb-latency-arbitrage-in-short-term-crypto-markets/)
- [Phemex - RN1 $2M Strategy](https://phemex.com/news/article/smart-money-rn1-nets-2m-on-polymarket-from-1k-investment-49297)
- [Phemex - Arbitrage Strategies](https://phemex.com/news/article/polymarket-arbitrage-strategies-for-crypto-traders-36382)
- [DataWallet - Top 10 Strategies](https://www.datawallet.com/crypto/top-polymarket-trading-strategies)
- [NPR - How Traders Make Money](https://www.npr.org/2026/01/17/nx-s1-5672615/kalshi-polymarket-prediction-market-boom-traders-slang-glossary)
- [MetaMask - Advanced Strategies](https://metamask.io/news/advanced-prediction-market-trading-strategies)
- [Medium - Portfolio Betting Agent](https://medium.com/@wanguolin/how-to-programmatically-identify-arbitrage-opportunities-on-polymarket-and-why-i-built-a-portfolio-23d803d6a74b)
- [QuantPedia - Systematic Edges](https://quantpedia.com/systematic-edges-in-prediction-markets/)
- [NYC Servers - Arbitrage Guide 2026](https://newyorkcityservers.com/blog/prediction-market-arbitrage-guide)
- [InvestX - RN1 Holy Trinity Strategy](https://investx.fr/en/crypto-news/polymarket-trader-turns-1000-into-2-million-unveiling-winning-strategy/)
- [LiveBitcoinNews - ilovecircle AI Trader](https://www.livebitcoinnews.com/this-polymarket-trader-made-2-2m-in-60-days-using-ai-heres-what-that-means-for-prediction-markets/)
- [ChainCatcher - Six Profit Models](https://www.chaincatcher.com/en/article/2233047)
- [WEEX - Top 10 Whales Analysis](https://www.weex.com/news/detail/dissecting-polymarkets-top-10-whales-27000-transactions-the-smart-money-mirage-and-the-law-of-survival-296753)

---

## Summary: Most Viable Strategies for 2026

| Strategy | Difficulty | Capital Req | Tech Req | Expected Return | Example |
|----------|------------|-------------|----------|-----------------|---------|
| **Holy Trinity Farming** | High | Low | High | 2000x (RN1) | Sports markets, synthetic sells |
| **AI Probability Models** | High | Medium | High | $2.2M/60 days | ilovecircle's Claude-based bot |
| Cross-Platform Arb | Medium | Medium | High | 5-20%/month | Kalshi/Polymarket spreads |
| Latency Arb | High | Low-Medium | Very High | Reduced by fees | BTC 15-min markets |
| Domain Expertise | Medium | Low | Low | Variable | Politics specialists |
| LP/Market Making | Medium | High | Medium | 80-200% APY | New market liquidity |
| Nearly Resolved ("Bonding") | Low | High | Low | 5%/72hrs | 95%+ probability events |
| Trash Farming | Low | Low | Low | Reward mining | End-of-event $0.01 buys |

### Key Takeaways

1. **RN1's Holy Trinity Farming** is the most documented successful strategy:
   - Never close positions (use synthetic sells)
   - Farm volume through "trash" contracts
   - Exploit AMM microstructure inefficiencies

2. **AI-assisted trading** (like ilovecircle) shows promise:
   - Compare model probability vs market price
   - Automate execution at scale
   - 74% accuracy achievable

3. **Pure latency arbitrage is harder** due to Polymarket's dynamic fees (~3.15% at 50% odds)

4. **Cross-platform arbitrage remains viable** but requires understanding different resolution criteria

5. **Sports markets** offer more frequent opportunities due to higher event frequency
