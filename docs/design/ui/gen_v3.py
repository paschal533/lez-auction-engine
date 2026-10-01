W,H=424,206
x0,x1=32,W-10; y0,yt=176,20          # plot box
X=lambda t: x0+(x1-x0)*t/120
Y=lambda d: y0-(y0-yt)*d/12
f=lambda v: f"{v:.1f}"
now_t=23.6; now_d=2+8*now_t/45
o=[f'<svg width="{W}" height="{H}" viewBox="0 0 {W} {H}" role="img" aria-label="Discount rises from 2% at the start to 10% at 45 minutes, then holds until the auction ends at 2 hours. Now 6.2% at 23 minutes 36 seconds.">']
for d in (2,4,6,8):
    o.append(f'<line x1="{x0}" y1="{f(Y(d))}" x2="{x1}" y2="{f(Y(d))}" stroke="#333333" stroke-dasharray="1 3"/>')
o.append(f'<line x1="{x0}" y1="{y0}" x2="{x1}" y2="{y0}" stroke="#434343"/>')
for d in (0,2,4,6,8,10):
    o.append(f'<text x="{x0-6}" y="{f(Y(d)+4)}" text-anchor="end">{d}%</text>')
for t,l in ((0,"0"),(15,"15m"),(30,"30m"),(45,"45m"),(60,"1h"),(90,"1h 30m"),(120,"2h")):
    anchor="start" if t==0 else ("end" if t==120 else "middle")
    o.append(f'<line x1="{f(X(t))}" y1="{y0}" x2="{f(X(t))}" y2="{y0+4}" stroke="#434343"/>')
    o.append(f'<text x="{f(X(t))}" y="{y0+18}" text-anchor="{anchor}">{l}</text>')
# max reference + expiry
o.append(f'<line x1="{x0}" y1="{f(Y(10))}" x2="{f(X(45))}" y2="{f(Y(10))}" stroke="rgba(237,123,88,.35)" stroke-dasharray="2 4"/>')
o.append(f'<line x1="{f(X(120))}" y1="{yt}" x2="{f(X(120))}" y2="{y0}" stroke="#5C5C5C" stroke-dasharray="3 3"/>')
o.append(f'<text x="{f(X(120)-6)}" y="{yt+4}" text-anchor="end" style="fill:#A4A4A4">Ends 16:02</text>')
o.append(f'<text x="{f(X(45))}" y="{f(Y(10)-10)}" text-anchor="middle" style="fill:#A4A4A4">Max at 14:47</text>')
# elapsed area + line
o.append(f'<polygon points="{x0},{y0} {x0},{f(Y(2))} {f(X(now_t))},{f(Y(now_d))} {f(X(now_t))},{y0}" fill="rgba(237,123,88,.16)"/>')
o.append(f'<polyline points="{x0},{f(Y(2))} {f(X(now_t))},{f(Y(now_d))}" fill="none" stroke="#ED7B58" stroke-width="2.5" stroke-linecap="round"/>')
o.append(f'<polyline points="{f(X(now_t))},{f(Y(now_d))} {f(X(45))},{f(Y(10))} {f(X(120))},{f(Y(10))}" fill="none" stroke="#ED7B58" stroke-opacity=".55" stroke-width="2" stroke-dasharray="5 4" stroke-linejoin="round"/>')
o.append(f'<circle cx="{f(X(45))}" cy="{f(Y(10))}" r="3" fill="#171717" stroke="#ED7B58" stroke-opacity=".8" stroke-width="1.5"/>')
# now
nx,ny=X(now_t),Y(now_d)
o.append(f'<line x1="{f(nx)}" y1="{f(ny)}" x2="{f(nx)}" y2="{y0}" stroke="#ED7B58"/>')
o.append(f'<circle cx="{f(nx)}" cy="{f(ny)}" r="8" fill="rgba(237,123,88,.22)"/>')
o.append(f'<circle cx="{f(nx)}" cy="{f(ny)}" r="4.5" fill="#ED7B58" stroke="#171717" stroke-width="2"/>')
o.append(f'<rect x="{f(nx+10)}" y="{f(ny+10)}" width="76" height="22" rx="4" fill="#232323" stroke="rgba(237,123,88,.4)"/>')
o.append(f'<text x="{f(nx+18)}" y="{f(ny+25)}" style="fill:#FFFFFF;font-weight:700">Now 6.2%</text>')
o.append('</svg>')
s=open('v3-template.html',encoding='utf-8').read().replace('{{CHART}}','\n                  '.join(o))
open('companion/.superpowers/brainstorm/469-1790873518/content/app-layout-v3.html','w',encoding='utf-8').write(s)
print("now",round(now_d,2))
