# Builds screens-v4.html from v4-screens-template.html + pieces of v3-template.html.
# Reuses v3's CSS, sidebar and (for the dialog backdrop) the whole v3 plugin view,
# so nothing in the shell can drift from the approved render.
import math, re

SRC = 'v3-template.html'
TPL = 'v4-screens-template.html'
OUT = 'companion/.superpowers/brainstorm/26491-1790875357/content/screens-v4.html'

v3 = open(SRC, encoding='utf-8').read()

# ---------- v3 chart (same code as gen_v3.py) ----------
W, H = 424, 206
x0, x1 = 32, W - 10; y0, yt = 176, 20
X = lambda t: x0 + (x1 - x0) * t / 120
Y = lambda d: y0 - (y0 - yt) * d / 12
f = lambda v: f"{v:.1f}"
now_t = 23.6; now_d = 2 + 8 * now_t / 45
o = [f'<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="Discount schedule">']
for d in (2, 4, 6, 8):
    o.append(f'<line x1="{x0}" y1="{f(Y(d))}" x2="{x1}" y2="{f(Y(d))}" stroke="#333333" stroke-dasharray="1 3"/>')
o.append(f'<line x1="{x0}" y1="{y0}" x2="{x1}" y2="{y0}" stroke="#434343"/>')
for d in (0, 2, 4, 6, 8, 10):
    o.append(f'<text x="{x0-6}" y="{f(Y(d)+4)}" text-anchor="end">{d}%</text>')
for t, l in ((0, "0"), (15, "15m"), (30, "30m"), (45, "45m"), (60, "1h"), (90, "1h 30m"), (120, "2h")):
    a = "start" if t == 0 else ("end" if t == 120 else "middle")
    o.append(f'<line x1="{f(X(t))}" y1="{y0}" x2="{f(X(t))}" y2="{y0+4}" stroke="#434343"/>')
    o.append(f'<text x="{f(X(t))}" y="{y0+18}" text-anchor="{a}">{l}</text>')
o.append(f'<line x1="{x0}" y1="{f(Y(10))}" x2="{f(X(45))}" y2="{f(Y(10))}" stroke="rgba(237,123,88,.35)" stroke-dasharray="2 4"/>')
o.append(f'<line x1="{f(X(120))}" y1="{yt}" x2="{f(X(120))}" y2="{y0}" stroke="#5C5C5C" stroke-dasharray="3 3"/>')
o.append(f'<text x="{f(X(120)-6)}" y="{yt+4}" text-anchor="end" style="fill:#A4A4A4">Ends 16:02</text>')
o.append(f'<text x="{f(X(45))}" y="{f(Y(10)-10)}" text-anchor="middle" style="fill:#A4A4A4">Max at 14:47</text>')
o.append(f'<polygon points="{x0},{y0} {x0},{f(Y(2))} {f(X(now_t))},{f(Y(now_d))} {f(X(now_t))},{y0}" fill="rgba(237,123,88,.16)"/>')
o.append(f'<polyline points="{x0},{f(Y(2))} {f(X(now_t))},{f(Y(now_d))}" fill="none" stroke="#ED7B58" stroke-width="2.5" stroke-linecap="round"/>')
o.append(f'<polyline points="{f(X(now_t))},{f(Y(now_d))} {f(X(45))},{f(Y(10))} {f(X(120))},{f(Y(10))}" fill="none" stroke="#ED7B58" stroke-opacity=".55" stroke-width="2" stroke-dasharray="5 4" stroke-linejoin="round"/>')
o.append(f'<circle cx="{f(X(45))}" cy="{f(Y(10))}" r="3" fill="#171717" stroke="#ED7B58" stroke-opacity=".8" stroke-width="1.5"/>')
nx, ny = X(now_t), Y(now_d)
o.append(f'<line x1="{f(nx)}" y1="{f(ny)}" x2="{f(nx)}" y2="{y0}" stroke="#ED7B58"/>')
o.append(f'<circle cx="{f(nx)}" cy="{f(ny)}" r="8" fill="rgba(237,123,88,.22)"/>')
o.append(f'<circle cx="{f(nx)}" cy="{f(ny)}" r="4.5" fill="#ED7B58" stroke="#171717" stroke-width="2"/>')
o.append(f'<rect x="{f(nx+10)}" y="{f(ny+10)}" width="76" height="22" rx="4" fill="#232323" stroke="rgba(237,123,88,.4)"/>')
o.append(f'<text x="{f(nx+18)}" y="{f(ny+25)}" style="fill:#FFFFFF;font-weight:700">Now 6.2%</text>')
o.append('</svg>')
chart = '\n'.join(o)

css = re.search(r'<style>\n(.*?)</style>', v3, re.S).group(1)
sidebar = re.search(r'(<nav class="sidebar".*?</nav>)', v3, re.S).group(1)
v3view = re.search(r'(<main class="view">.*?</main>)', v3, re.S).group(1).replace('{{CHART}}', chart)
# The backdrop is decorative: hide it from assistive tech, the dialog is what is focused.
v3view = v3view.replace('<main class="view">', '<main class="view" aria-hidden="true">', 1)


# ---------- LogosStageLane, ported from LogosStageLane.qml ----------
FILL = {  # d.fillFor(state)
    'done': '#343434',                      # palette.surface
    'busy': 'rgba(254,188,46,.21)',         # warning @ 0.14 + 0.14*pulse (mid-pulse)
    'active': 'rgba(73,245,99,.20)',        # success @ 0.20
    'failed': 'rgba(251,55,72,.22)',        # error @ 0.22
    'pending': '#1F1F1F',                   # palette.surfaceRecessed = gray370
}
TEXT = {'done': '#A4A4A4', 'busy': '#FEBC2E', 'active': '#49F563', 'failed': '#FB3748', 'pending': '#969696'}
NOTCH, GAP, CR = 14, 2, 3


def chevron(i, n, w, h, dep):
    pts = [(0, 0)]
    if i == n - 1:
        pts += [(w, 0), (w, h)]
    else:
        pts += [(w - dep, 0), (w, h / 2), (w - dep, h)]
    pts.append((0, h))
    if i > 0:
        pts.append((dep, h / 2))
    out = ''
    m = len(pts)
    for k in range(m):
        p, c, nx_ = pts[k - 1], pts[k], pts[(k + 1) % m]
        il = math.hypot(c[0] - p[0], c[1] - p[1]); ol = math.hypot(nx_[0] - c[0], nx_[1] - c[1])
        r = min(CR, il / 2, ol / 2)
        fr = (c[0] + (p[0] - c[0]) * r / il, c[1] + (p[1] - c[1]) * r / il)
        to = (c[0] + (nx_[0] - c[0]) * r / ol, c[1] + (nx_[1] - c[1]) * r / ol)
        out += ('M ' if k == 0 else 'L ') + f'{fr[0]:.2f} {fr[1]:.2f} Q {c[0]:.2f} {c[1]:.2f} {to[0]:.2f} {to[1]:.2f} '
    return out + 'Z'


def lane(width, stages, h=30):
    n = len(stages)
    dep = min(NOTCH, h * 0.5)
    sw = (width + (n - 1) * (dep - GAP)) / n
    left = lambda i: i * (sw - dep + GAP)
    s = [f'<svg class="lane" width="{width}" height="{h}" viewBox="0 0 {width} {h}" role="img" '
         f'aria-label="{" , ".join(l + " (" + st + ")" for l, st in stages)}">']
    for i, (label, st) in enumerate(stages):
        s.append(f'<path transform="translate({left(i):.2f} 0)" d="{chevron(i, n, sw, h, dep)}" fill="{FILL[st]}"/>')
    for i, (label, st) in enumerate(stages):
        lft = left(i) + (dep if i > 0 else 0)
        rgt = left(i) + sw - (dep if i < n - 1 else 0)
        cx = (lft + rgt) / 2
        tick = f'<tspan fill="#49F563" font-weight="700">✓ </tspan>' if st == 'done' else ''
        s.append(f'<text x="{cx:.1f}" y="{h/2+4:.1f}" text-anchor="middle" fill="{TEXT[st]}">{tick}{label}</text>')
    s.append('</svg>')
    return ''.join(s)


LANES = {
    'WAIT':   lane(251, [('Submitted', 'done'), ('Including…', 'busy'), ('Done', 'pending')]),
    'DONE':   lane(251, [('Submitted', 'done'), ('Included', 'done'), ('Done', 'active')]),
    'FAIL':   lane(251, [('Submitted', 'done'), ('Not included', 'failed'), ('Done', 'pending')]),
    'PRIV':   lane(251, [('Deshield', 'done'), ('Bidding…', 'busy'), ('Reshield', 'pending')]),
}

page = open(TPL, encoding='utf-8').read()
page = page.replace('{{CSS_V3}}', css).replace('{{SIDEBAR}}', sidebar).replace('{{V3VIEW}}', v3view)
for k, v in LANES.items():
    page = page.replace('{{LANE_' + k + '}}', v)
left = re.findall(r'\{\{[A-Z_0-9]+\}\}', page)
assert not left, left
open(OUT, 'w', encoding='utf-8').write(page)
print('wrote', OUT, len(page))
