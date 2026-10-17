"""Which token the keyless lane's fake model answers a turn with: a wrong
pick lets one turn's token pass another turn's cell."""

import json
from pathlib import Path
import sys
import unittest

sys.path.insert(0, str(Path(__file__).resolve().parents[1]))
from agent_journey import fake_model

INDEX = '<hide-memory-index trust="untrusted-reference-data">\nm7 Marker JOURNEY-0000AAAA for journey checks\n</hide-memory-index>'
CONTEXT = '<hide-memory-context trust="untrusted-reference-data">\n{"id":"m7","text":"Marker JOURNEY-0000BBBB"}\n</hide-memory-context>'


def messages(*turns):
    """An Anthropic Messages request of (role, text) turns."""
    return {"messages": [{"role": role, "content": [{"type": "text", "text": text}]} for role, text in turns]}


class Answer(unittest.TestCase):
    def test_a_later_turn_never_answers_with_an_earlier_turns_token(self):
        request = messages(("user", INDEX + "\nReply with exactly JOURNEY-1111AAAA"), ("assistant", "JOURNEY-1111AAAA"),
                           ("user", "Reply with exactly the code in the title of your only Project Memory card."))
        self.assertEqual(fake_model.answer(request), "JOURNEY-0000AAAA")

    def test_the_turns_own_token_wins_over_the_memory_blocks_it_carries(self):
        request = messages(("user", INDEX + "\n" + CONTEXT + "\nReply with exactly JOURNEY-2222AAAA"))
        self.assertEqual(fake_model.answer(request), "JOURNEY-2222AAAA")

    def test_a_card_a_prompt_hook_adds_is_not_the_index(self):
        request = messages(("user", "hello"), ("assistant", "OK"), ("user", CONTEXT + "\nWhat is the code?"))
        self.assertEqual(fake_model.answer(request), "OK")

    def test_a_letter_handed_over_with_the_prompt_answers_its_token(self):
        request = {"input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "JOURNEY-3333AAAA"}]},
                             {"type": "message", "role": "assistant", "content": [{"type": "output_text", "text": "JOURNEY-3333AAAA"}]},
                             {"type": "message", "role": "developer", "content": [{"type": "input_text", "text": "Letter\nJOURNEY-4444AAAA"}]},
                             {"type": "message", "role": "user", "content": [{"type": "input_text", "text": "Reply with the letter's token."}]}]}
        self.assertEqual(fake_model.answer(request), "JOURNEY-4444AAAA")

    def test_gemini_turns_end_at_the_models_last_content(self):
        request = {"contents": [{"role": "user", "parts": [{"text": "JOURNEY-5555AAAA"}]},
                                {"role": "model", "parts": [{"text": "JOURNEY-5555AAAA"}]},
                                {"role": "user", "parts": [{"text": "no token"}]}]}
        self.assertEqual(fake_model.answer(request), "OK")


class Schema(unittest.TestCase):
    def test_a_title_request_is_answered_with_its_schemas_object(self):
        # The shapes Claude Code 2.1.302 and Codex 0.163 send for a session title.
        claude = {**messages(("user", "<session>\nReply with exactly JOURNEY-6666AAAA\n</session>")),
                  "output_config": {"format": {"type": "json_schema", "schema": {
                      "type": "object", "properties": {"title": {"type": "string"}}, "required": ["title"]}}}}
        codex = {"input": [{"type": "message", "role": "user", "content": [{"type": "input_text", "text": "JOURNEY-6666AAAA"}]}],
                 "text": {"format": {"type": "json_schema", "name": "codex_output_schema", "schema": {
                     "type": "object", "properties": {"title": {"type": "string", "minLength": 1, "maxLength": 12}},
                     "required": ["title"]}}}}
        self.assertEqual(json.loads(fake_model.reply(claude)[0]), {"title": "Journey JOURNEY-6666AAAA"})
        self.assertEqual(json.loads(fake_model.reply(codex)[0]), {"title": "Journey JOUR"})

    def test_a_plain_request_is_answered_with_the_token(self):
        self.assertEqual(fake_model.reply(messages(("user", "JOURNEY-7777AAAA"))), ("JOURNEY-7777AAAA", "JOURNEY-7777AAAA"))


if __name__ == "__main__":
    unittest.main()
