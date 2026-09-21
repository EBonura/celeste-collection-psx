#!/usr/bin/env python3
"""Keep linked-list primitives within Sony's documented 16-word total size.

Fixture is one completed Classic frame from the unchanged 0.2.3 executable.
No assets, executable code or RAM outside its GPU command list are included.
"""
import json
from pathlib import Path
import unittest

ROOT = Path(__file__).resolve().parents[1]
SAFE_PAYLOAD_WORDS = 15  # 16 total words includes the linked-list tag.


def packets(words):
    at = 0
    while at < len(words):
        opcode = words[at] >> 24
        if opcode == 0xA0:
            wh = words[at + 2]
            width = ((wh - 1) & 1023) + 1
            height = (((wh >> 16) - 1) & 511) + 1
            size = 3 + (width * height + 1) // 2
        elif opcode in (0x00, 0x01, 0x1F) or 0xE1 <= opcode <= 0xE6:
            size = 1
        elif opcode == 0x02:
            size = 3
        elif 0x20 <= opcode <= 0x3F:
            vertices = 4 if opcode & 8 else 3
            size = 1 + vertices + (vertices if opcode & 4 else 0)
            size += vertices - 1 if opcode & 16 else 0
        elif 0x60 <= opcode <= 0x7F:
            size = 2 + bool(opcode & 4) + ((opcode & 0x18) == 0)
        else:
            raise ValueError(f"Unsupported fixture opcode {opcode:02x}")
        if at + size > len(words):
            raise ValueError("GPU packet crosses node boundary")
        yield words[at:at + size]
        at += size


def regroup(commands, limit):
    result, current = [], []
    for command in commands:
        if len(command) > limit:
            raise ValueError("A complete GPU packet does not fit")
        if len(current) + len(command) > limit:
            result.append(current)
            current = []
        current.extend(command)
    if current:
        result.append(current)
    return result


class GpuListContract(unittest.TestCase):
    def setUp(self):
        self.nodes = json.loads((ROOT / "tools/fixtures/gpu-list-classic-frame.json").read_text())
        self.commands = [packet for node in self.nodes for packet in packets(node["words"])]

    def test_backend_uses_sdk_ordered_stream(self):
        source = (ROOT / "shared/src/backend.rs").read_text()
        self.assertIn("gpu::ordered::OrderedCommandStream", source)
        self.assertNotIn("const NODE_MAX", source)
        self.assertNotIn("fn kick_pending", source)

    def test_shipped_fixture_demonstrates_violation(self):
        self.assertEqual([node["count"] for node in self.nodes], [254, 253, 252, 236])
        self.assertTrue(all(node["count"] > SAFE_PAYLOAD_WORDS for node in self.nodes))
        self.assertEqual(len(self.commands), 264)

    def test_regroup_preserves_every_word_and_packet(self):
        small = regroup(self.commands, SAFE_PAYLOAD_WORDS)
        self.assertTrue(all(len(node) <= SAFE_PAYLOAD_WORDS for node in small))
        self.assertEqual([word for node in small for word in node],
                         [word for node in self.nodes for word in node["words"]])
        self.assertEqual([packet for node in small for packet in packets(node)], self.commands)

    def test_exact_limit_and_oversize_packet(self):
        self.assertEqual(list(map(len, regroup([[0] * 11, [0] * 4, [0]], 15))), [15, 1])
        with self.assertRaises(ValueError):
            regroup([[0] * 16], 15)

    def test_upload_cannot_be_split(self):
        upload = next(packet for packet in self.commands if packet[0] >> 24 == 0xA0)
        self.assertEqual(len(upload), 11)
        self.assertEqual(regroup([[0] * 5, upload], 15), [[0] * 5, upload])
        with self.assertRaises(ValueError):
            list(packets(upload[:-1]))


if __name__ == "__main__":
    unittest.main()
