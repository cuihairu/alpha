#!/usr/bin/env python3
import argparse
import json
import sys
import urllib.parse
import urllib.request
from datetime import datetime, timezone


def _guess_secid(symbol: str) -> str:
    s = symbol.lower().strip()
    if s.startswith(("sh", "sz", "bj")):
        s = s[2:]

    # Eastmoney: 0=SZ, 1=SH, 4=BJ
    if s.startswith(("60", "68", "51")):
        market = "1"
    elif s.startswith(("00", "30")):
        market = "0"
    elif s.startswith(("43", "83", "87")):
        market = "4"
    else:
        market = "0"
    return f"{market}.{s}"


def fetch_quotes(symbols: list[str], timeout: float) -> dict:
    secids = [_guess_secid(s) for s in symbols]
    query = {
        "action": "fl",
        "fields": "f12,f13,f14,f2,f3,f4,f5,f6,f15,f16,f17,f18,f124",
        "fltt": "2",
        "secids": ",".join(secids),
    }
    url = "https://push2.eastmoney.com/api/qt/ulist.np/get?" + urllib.parse.urlencode(query)

    req = urllib.request.Request(
        url,
        headers={
            "Accept": "application/json",
            "Referer": "https://quote.eastmoney.com/",
            "User-Agent": "Mozilla/5.0 (compatible; alpha-crawlers/0.1; +https://alpha.finance)",
        },
        method="GET",
    )

    with urllib.request.urlopen(req, timeout=timeout) as resp:
        raw = resp.read().decode("utf-8", errors="replace")
        data = json.loads(raw)

    diff = (data.get("data") or {}).get("diff") or []
    quotes = []
    for item in diff:
        code = str(item.get("f12") or "")
        market = item.get("f13")
        secid = f"{market}.{code}" if market is not None and code else None

        price = float(item.get("f2") or 0.0)
        if not code or price <= 0:
            continue

        ts = item.get("f124") or 0
        if isinstance(ts, (int, float)) and ts > 0:
            timestamp = datetime.fromtimestamp(int(ts), tz=timezone.utc).isoformat()
        else:
            timestamp = datetime.now(tz=timezone.utc).isoformat()

        quotes.append(
            {
                "symbol": code,
                "secid": secid,
                "name": item.get("f14") or "",
                "price": price,
                "pre_close": float(item.get("f18") or 0.0),
                "open": float(item.get("f17") or 0.0),
                "high": float(item.get("f15") or 0.0),
                "low": float(item.get("f16") or 0.0),
                "volume": int(item.get("f5") or 0),
                "amount": float(item.get("f6") or 0.0),
                "change": float(item.get("f4") or 0.0),
                "change_percent": float(item.get("f3") or 0.0),
                "timestamp": timestamp,
                "source": "eastmoney",
            }
        )

    return {"source": "eastmoney", "quotes": quotes, "requested": symbols}


def main() -> int:
    parser = argparse.ArgumentParser(description="Fetch A-share realtime quotes from Eastmoney (no deps).")
    parser.add_argument("--symbols", required=True, help="Comma-separated symbols, e.g. 000001,600000,600519")
    parser.add_argument("--timeout", type=float, default=10.0)
    parser.add_argument("--pretty", action="store_true", help="Pretty-print JSON")
    args = parser.parse_args()

    symbols = [s.strip() for s in args.symbols.split(",") if s.strip()]
    if not symbols:
        print("missing symbols", file=sys.stderr)
        return 2

    result = fetch_quotes(symbols, timeout=args.timeout)
    if args.pretty:
        print(json.dumps(result, ensure_ascii=False, indent=2))
    else:
        print(json.dumps(result, ensure_ascii=False, separators=(",", ":")))
    return 0


if __name__ == "__main__":
    raise SystemExit(main())

