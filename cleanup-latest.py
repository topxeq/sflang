# -*- coding: utf-8 -*-
"""Cleanup old sf (Sflang) versions on magicdo.top — xxssh 成熟模式:
unset isLatest first (delete is refused otherwise), then DELETE old version
entries entirely. Each platform keeps ONLY the current release.

Usage: python cleanup-latest.py <version-to-keep>   (called by publish-*.ps1)
"""
import json
import os
import sys
import urllib.request

BASE = 'https://magicdo.top'
PRODUCT_ID = 'sf'
KEEP = sys.argv[1] if len(sys.argv) > 1 else '0.1.0'

# 凭据从环境变量读取（仓库不含明文）：MAGICDO_USERNAME 缺省 topget，MAGICDO_PASSWORD 必填
MAGICDO_USER = os.environ.get('MAGICDO_USERNAME', 'topget')
MAGICDO_PASSWORD = os.environ.get('MAGICDO_PASSWORD')
assert MAGICDO_PASSWORD, '请先设置环境变量 MAGICDO_PASSWORD（magicdo.top 管理密码）'

def post(url, data=None):
    body = json.dumps(data).encode() if data is not None else None
    req = urllib.request.Request(url, data=body, method='POST',
                                 headers={'Content-Type': 'application/json'})
    return urllib.request.urlopen(req, timeout=60).read().decode('utf-8')

def request(url, method):
    req = urllib.request.Request(url, method=method)
    return urllib.request.urlopen(req, timeout=60).read().decode('utf-8')

def get(url):
    return urllib.request.urlopen(url, timeout=60).read().decode('utf-8')

resp = json.loads(post(f'{BASE}/api/admin/auth',
                       {'username': MAGICDO_USER, 'password': MAGICDO_PASSWORD}))
assert resp.get('success'), resp
token = resp['token']

def fetch_versions():
    prod = json.loads(get(f'{BASE}/api/products?id={PRODUCT_ID}'))
    return prod.get('product', prod).get('versions') or []

vers = fetch_versions()
print(f'version entries: {len(vers)} (keeping only {KEEP})')

unset = deleted = 0
for v in vers:
    if v.get('version') == KEEP:
        continue
    vid = v['id']
    if v.get('isLatest'):
        r = json.loads(post(
            f'{BASE}/api/admin/products?token={token}&action=updateVersion'
            f'&id={PRODUCT_ID}&vid={vid}', {'isLatest': False}))
        ok = r.get('success', False)
        print(f"  unset {v.get('platform'):9s} {v.get('version'):7s} -> {'OK' if ok else r}")
        unset += ok
    r = json.loads(request(
        f'{BASE}/api/admin/products?token={token}&action=version'
        f'&id={PRODUCT_ID}&vid={vid}&confirm=DELETE-VERSION', 'DELETE'))
    ok = r.get('success', False)
    print(f"  delete {v.get('platform'):9s} {v.get('version'):7s} -> {'OK' if ok else r}")
    deleted += ok

remaining = fetch_versions()
print(f'done: unset {unset}, deleted {deleted}; remaining: '
      f'{[(v["platform"], v["version"]) for v in remaining]}')
