# Builds auctions-settled-system-dev-v5.html from v5-screens-template.html.
# Pulls v3's CSS and sidebar out of v3-template.html and v4's additions out of
# v4-screens-template.html, so the shell and every v4 component stay identical
# to the approved renders. Run from this directory: python gen_v5.py
import re

v3 = open('v3-template.html', encoding='utf-8').read()
v4 = open('v4-screens-template.html', encoding='utf-8').read()

css_v3 = re.search(r'<style>\n(.*?)</style>', v3, re.S).group(1)
sidebar = re.search(r'(<nav class="sidebar".*?</nav>)', v3, re.S).group(1)
# v4's own block: everything after its {{CSS_V3}} placeholder up to </style>.
css_v4 = re.search(r'\{\{CSS_V3\}\}\n(.*?)</style>', v4, re.S).group(1)

page = open('v5-screens-template.html', encoding='utf-8').read()
page = page.replace('{{CSS_V3}}', css_v3).replace('{{CSS_V4}}', css_v4).replace('{{SIDEBAR}}', sidebar)
left = re.findall(r'\{\{[A-Z_0-9]+\}\}', page)
assert not left, left
OUT = 'auctions-settled-system-dev-v5.html'
open(OUT, 'w', encoding='utf-8').write(page)
print('wrote', OUT, len(page))
