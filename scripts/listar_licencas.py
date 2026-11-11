#!/usr/bin/env python3
"""Lista as dependências Rust (nome, versão, licença) usadas pelo Ignis.
Uso: cargo metadata --format-version 1 | python3 scripts/listar_licencas.py > AVISOS-DEPENDENCIAS.txt
"""
import json
import sys

d = json.load(sys.stdin)
pk = {p["id"]: p for p in d["packages"]}
nodes = {n["id"]: n for n in d["resolve"]["nodes"]}
stack = [p["id"] for p in d["packages"] if p["source"] is None]
seen = set()
while stack:
    i = stack.pop()
    if i in seen:
        continue
    seen.add(i)
    for dep in nodes[i]["deps"]:
        if any(k["kind"] in (None, "build") for k in dep["dep_kinds"]):
            stack.append(dep["pkg"])
linhas = sorted(
    (pk[i]["name"], pk[i]["version"], pk[i].get("license") or "ver repositório")
    for i in seen
    if pk[i]["source"] is not None
)
print("Dependências Rust do Ignis (geradas por scripts/listar_licencas.py)\n")
for nome, versao, lic in linhas:
    print(f"{nome} {versao} — {lic}")
