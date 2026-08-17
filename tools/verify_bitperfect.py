#!/usr/bin/env python3
"""Compara o dump do player com o PCM de referência extraído pelo ffmpeg.

O dump contém float32 — o formato que o device entrega ao IOProc; a referência contém
inteiros no bit depth original do arquivo. A comparação converte o float de volta para
inteiro: se a ida e volta for exata, como o projeto afirma, os valores têm de bater
exatamente, amostra por amostra.
"""
import argparse
import struct
import sys


def read_int_samples(path, bits):
    """Lê PCM inteiro little-endian empacotado (3 bytes por amostra para 24 bits)."""
    width = bits // 8
    with open(path, "rb") as handle:
        data = handle.read()
    usable = len(data) - (len(data) % width)
    return [
        int.from_bytes(data[i:i + width], "little", signed=True)
        for i in range(0, usable, width)
    ]


def read_float_samples(path):
    with open(path, "rb") as handle:
        data = handle.read()
    count = len(data) // 4
    return list(struct.unpack(f"<{count}f", data[:count * 4]))


def main():
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("reference", help="PCM inteiro gerado pelo ffmpeg")
    parser.add_argument("dump", help="float32 gravado pelo player com --dump")
    parser.add_argument("--bits", type=int, default=24, help="bit depth da referência")
    args = parser.parse_args()

    reference = read_int_samples(args.reference, args.bits)
    dump = read_float_samples(args.dump)

    if not reference or not dump:
        print("ERRO: um dos arquivos está vazio")
        return 1

    scale = float(1 << (args.bits - 1))
    # O dump pode terminar com um bloco completado por zeros: comparar até o menor dos dois
    # evita acusar divergência por causa do preenchimento final.
    count = min(len(reference), len(dump))

    mismatches = 0
    first = None
    for i in range(count):
        converted = int(round(dump[i] * scale))
        if converted != reference[i]:
            mismatches += 1
            if first is None:
                first = (i, reference[i], dump[i], converted)

    print(f"amostras comparadas : {count}")
    print(f"referência          : {len(reference)} amostras ({args.bits} bits)")
    print(f"dump                : {len(dump)} amostras (float32)")

    if mismatches == 0:
        print("resultado           : BIT-PERFECT — todas as amostras batem exatamente")
        return 0

    index, expected, raw, converted = first
    print(f"resultado           : DIVERGE em {mismatches} de {count} amostras")
    print(f"primeira divergência: índice {index}, referência {expected}, "
          f"dump {raw!r} -> {converted}")
    return 1


if __name__ == "__main__":
    sys.exit(main())
