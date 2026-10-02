"""Independent big-integer reference for auction_core (no shared code with the Rust crate)."""
RAY = 10**27
BPS_TO_RAY = 10**23
U128 = 2**128 - 1


def mul_div_floor(x, y, d):
    q = (x * y) // d
    return q if q <= U128 else None


def mul_div_ceil(x, y, d):
    q = -((-(x * y)) // d)
    return q if q <= U128 else None


# NOTE: these points are pinned to `schedule::rai_default()` in the Rust crate. A mismatch after
# changing rai_default is the intended failure: update this model deliberately alongside it.
def rai_discount(t):
    if t >= 2_700_000:
        return 1000 * BPS_TO_RAY
    return 200 * BPS_TO_RAY + (800 * BPS_TO_RAY * t) // 2_700_000


def bid(coin, t, price, lot, remaining):
    """Return (collateral_out, charged) for a fresh auction holding `lot` collateral.

    Caps the bid at the remaining target, floors the fill, binds it to the lot, and
    applies the final-fill exception: a bid reaching the remaining target whose plain
    floor fill is 0 still takes 1 unit of collateral and is charged in full.
    """
    coin = min(coin, remaining)
    denom = RAY - rai_discount(t)
    full_out = (coin * price) // denom
    if full_out >= lot:
        return lot, min(-((-(lot * denom)) // price), coin)
    if coin == remaining and full_out == 0 and lot > 0:
        return 1, coin
    return full_out, coin
