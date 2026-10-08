"""Alpha-aware 4bpp quantiser behind host/cook.py Atlas._quantize.

Same contract as the cook's: returns (resized image, 32-byte palette, index plane)
with index 0 = 0x0000 transparent, and the same three texel classes the cook
already has, decided per PIXEL instead of per octree centroid:

  alpha < 16          index 0, transparent (unchanged rule)
  alpha >= 224        opaque entry: straight colour, STP clear, black stored as 0x0001
  16 <= alpha < 224   semi entry: STP set, premultiplied colour (a*c), which the
                      scenery/actor path draws with the Add blend: out = bg + a*c

with one exception, the anti-aliased EDGE of opaque art: a semi pixel within two
texels of an opaque one. Add is exact only over black (its error is a*bg), so an
edge texel is instead given whichever of transparent / opaque-composited /
semi has the least expected error over a dark background prior (HK_DARK_PRIOR);
an opaque edge texel stores the source composited over the prior's mean. Semi
texels away from opaque art (smoke, glow, fog, renderer-faded layers) keep the
semi class, so things behind them still show through; only an interior texel
darker than the background it would be added to is dropped (Add cannot darken).

The 15 entries are split between the opaque and semi classes by greedy SSE
reduction, each class is weighted k-means in the space the GPU actually
displays (straight colour for opaque, premultiplied for semi), dark outline
pixels count six times (they carry the art), centroids are snapped to 15-bit
and every pixel is re-assigned to its nearest snapped entry of its own class.
A texel can therefore never change class: faded or smoky art stays
semi-transparent, and an opaque texel never becomes a see-through one.

Pure-black art (every visible texel black) keeps the cook's own quantiser so
black-mask palettes and their material admission stay byte-identical.
"""
import struct, hashlib
import numpy as np
from PIL import Image

INK_LUMA, INK_WEIGHT = 48, 6.0
LUMA = np.array([.299, .587, .114])
SAMPLE, ITERS = 8000, 16
# Background prior for edge texels: dark blue-greys typical of HK rooms (tunable).
HK_DARK_PRIOR = np.array([[20, 22, 30], [45, 50, 65], [90, 95, 110]], np.float64)
EDGE_REACH = 2

def _assign(x, c):
    d = (x*x).sum(1)[:, None] - 2*x@c.T + (c*c).sum(1)[None, :]
    return d.argmin(1), d.min(1)

def _wkmeans(x, w, k, rng):
    n = len(x)
    if n == 0: return np.zeros((0, 3)), 0.0
    if n <= k:
        return x.copy(), 0.0
    c = [x[rng.choice(n, p=w/w.sum())]]
    d2 = ((x-c[0])**2).sum(1)
    for _ in range(1, k):
        p = d2*w
        if p.sum() <= 0: break
        c.append(x[rng.choice(n, p=p/p.sum())]); d2 = np.minimum(d2, ((x-c[-1])**2).sum(1))
    c = np.array(c)
    for _ in range(ITERS):
        lab, _ = _assign(x, c)
        nc = c.copy()
        for i in range(len(c)):
            m = lab == i
            if m.any(): nc[i] = np.average(x[m], 0, w[m])
        if np.allclose(nc, c): break
        c = nc
    _, d = _assign(x, c)
    return c, float((np.maximum(d, 0)*w).sum())

def _sample(x, w, rng):
    if len(x) > SAMPLE:
        i = rng.choice(len(x), SAMPLE, replace=False); return x[i], w[i]*(len(x)/SAMPLE)
    return x, w

def to15(c):
    r, g, b = [int(min(255, max(0, round(v)))) >> 3 for v in c]
    return r | (g << 5) | (b << 10)

def expand(word):
    v = np.array([word & 31, (word >> 5) & 31, (word >> 10) & 31], np.float64)
    return v*8 + v/4  # (v<<3)|(v>>2)

def quantize(im, w, h, fallback=None):
    """-> (resized RGBA image, 32-byte palette, flat plane of w*h indices)."""
    im = im.convert('RGBA').resize((w, h), Image.Resampling.LANCZOS)
    a = np.asarray(im, np.float64).reshape(-1, 4)
    rgb, al = a[:, :3], a[:, 3]
    T = al < 16; O = al >= 224; S = ~T & ~O
    # Pure-black art keeps the caller's path. Decide it on the source texels,
    # before the edge pass below blends a background into any of them.
    if fallback is not None and (~T).any() and not rgb[~T].any():
        return fallback(im, w, h)
    rgb = rgb.copy()
    if EDGE_REACH and O.any() and S.any():
        o2 = O.reshape(h, w); near = o2.copy()
        for dy in range(-EDGE_REACH, EDGE_REACH+1):
            for dx in range(-EDGE_REACH, EDGE_REACH+1):
                sh = np.zeros_like(o2)
                ys, yd = (slice(max(0, dy), h+min(0, dy)), slice(max(0, -dy), h+min(0, -dy)))
                xs, xd = (slice(max(0, dx), w+min(0, dx)), slice(max(0, -dx), w+min(0, -dx)))
                sh[yd, xd] = o2[ys, xs]; near |= sh
        E = S & near.reshape(-1)
        if E.any():
            ae = al[E, None]/255.0; ce = rgb[E]; B = HK_DARK_PRIOR; bm = B.mean(0)
            cT = ((ae[:, None, :]*(ce[:, None, :]-B[None]))**2).sum(2).mean(1)
            cO = (((1-ae)[:, None, :]*(B[None]-bm))**2).sum(2).mean(1)
            cS = ((ae[:, None, :]*B[None])**2).sum(2).mean(1)
            pick = np.stack([cT, cO, cS], 1).argmin(1)
            idx = np.nonzero(E)[0]
            T[idx[pick == 0]] = True; S[idx[pick == 0]] = False
            to_o = idx[pick == 1]
            rgb[to_o] = ae[pick == 1]*ce[pick == 1] + (1-ae[pick == 1])*bm
            O[to_o] = True; S[to_o] = False
    # Interior semi texels never become opaque, but Add can only brighten: where
    # a texel is darker than the background it covers (dark smoke), dropping it
    # costs less than adding it, so it goes transparent instead.
    I = S.copy()
    if I.any():
        ai = al[I, None]/255.0; ci = rgb[I]; B = HK_DARK_PRIOR
        cT = ((ai[:, None, :]*(ci[:, None, :]-B[None]))**2).sum(2).mean(1)
        cS = ((ai[:, None, :]*B[None])**2).sum(2).mean(1)
        drop = np.nonzero(I)[0][cT < cS]
        T[drop] = True; S[drop] = False
    if fallback is not None and (~T).any() and not rgb[~T].any():
        return fallback(im, w, h)
    rng = np.random.default_rng(int.from_bytes(hashlib.sha256(a.tobytes()).digest()[:8], 'little'))
    xo = rgb[O]; wo = np.where(xo @ LUMA < INK_LUMA, INK_WEIGHT, 1.0)
    xs = rgb[S]*(al[S, None]/255.0); ws = np.ones(len(xs))
    so, swo = _sample(xo, wo, rng); ss, sws = _sample(xs, ws, rng)
    curves = {'o': {}, 's': {}}
    def curve(cls, k):
        if k not in curves[cls]:
            curves[cls][k] = _wkmeans(*((so, swo) if cls == 'o' else (ss, sws)), k, rng)
        return curves[cls][k]
    alloc = {'o': int(len(xo) > 0), 's': int(len(xs) > 0)}
    budget = 15 - sum(alloc.values())
    for _ in range(budget):
        best, gain = None, -1.0
        for cls, n in (('o', len(xo)), ('s', len(xs))):
            k = alloc[cls]
            if k == 0 or k >= n: continue
            g = curve(cls, k)[1] - curve(cls, k+1)[1]
            if g > gain: best, gain = cls, g
        if best is None: break
        alloc[best] += 1
    words = [0]; plane = np.zeros(w*h, np.int64)
    for cls, mask, x in (('o', O, xo), ('s', S, xs)):
        if alloc[cls] == 0: continue
        cents = curve(cls, alloc[cls])[0]
        cw = [to15(c) for c in cents]
        cw = [(v or 1) if cls == 'o' else (v | 0x8000) for v in cw]
        # dedupe identical snapped words, then re-assign in displayed colour
        uniq = list(dict.fromkeys(cw))
        disp = np.array([expand(v & 0x7fff) for v in uniq])
        lab, _ = _assign(x, disp)
        base = len(words); words.extend(uniq)
        plane[mask] = lab + base
    words += [0]*(16-len(words))
    return im, struct.pack('<16H', *words), plane.tolist()
