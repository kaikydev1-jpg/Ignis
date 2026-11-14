#!/usr/bin/env python3
"""Gera o atualizacoes.json publicado junto com cada versão (GitHub Releases).

Traz três coisas que atualizam cada uma no seu ritmo:
  app       -> versão do aplicativo e página de download
  motores   -> manifest/motores.json (llama.cpp fixado com SHA-256)
  catalogo  -> manifest/catalogo.json
Uso: gerar_atualizacoes.py VERSAO [saida.json]
"""
import json
import pathlib
import sys

raiz = pathlib.Path(__file__).resolve().parent.parent
versao = sys.argv[1].lstrip("v")
saida = pathlib.Path(sys.argv[2] if len(sys.argv) > 2 else "atualizacoes.json")
arquivo_resumo = raiz / ".github/release/resumo.txt"
resumo = arquivo_resumo.read_text(encoding="utf-8").strip() if arquivo_resumo.exists() else ""
dados = {
    "versao_manifesto": 1,
    "app": {
        "versao": versao,
        "pagina": f"https://github.com/kaikydev1-jpg/Ignis/releases/tag/v{versao}",
        # uma linha do que mudou: aparece na faixa "versão nova" da janela
        "notas": resumo,
    },
    "motores": json.loads((raiz / "manifest/motores.json").read_text(encoding="utf-8")),
    "catalogo": json.loads((raiz / "manifest/catalogo.json").read_text(encoding="utf-8")),
}
saida.write_text(json.dumps(dados, ensure_ascii=False, indent=2), encoding="utf-8")
print(f"gerado {saida}")
