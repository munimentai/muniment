import importlib.util
from pathlib import Path
import unittest

spec = importlib.util.spec_from_file_location('classifier_server', Path(__file__).with_name('serve-classifiers.py'))
server = importlib.util.module_from_spec(spec)
spec.loader.exec_module(server)


class ClassifierContract(unittest.TestCase):
    def setUp(self):
        self.body = {'model': 'test', 'state': 'Write a greeting.', 'questions': {'route': {
            'type': 'choice', 'instructions': 'Which model?', 'criteria': {'fast': 'Writing.', 'code': 'Programming.'}}}}

    def test_explicit_confidence_and_original_route_ids(self):
        result = server.classify(self.body, lambda *_: {'fast': .8, 'code': .2}, 'test')
        self.assertEqual(result['answers']['route']['choice'], 'fast')
        self.assertEqual(result['answers']['route']['confidence'], .8)

    def test_rejects_invalid_distributions(self):
        for probs in [{'fast': 1}, {'fast': float('nan'), 'code': 0}, {'fast': .9, 'code': .9}, {'fast': 2, 'code': -1}]:
            with self.subTest(probs=probs), self.assertRaises(ValueError):
                server.classify(self.body, lambda *_: probs, 'test')

    def test_rejects_unknown_model_and_oversized_state_before_inference(self):
        for change in [{'model': 'other'}, {'state': 'x' * 8001}, {'questions': {}}]:
            with self.subTest(change=change), self.assertRaises(ValueError):
                server.classify(dict(self.body, **change), lambda *_: self.fail('must not infer'), 'test')


if __name__ == '__main__':
    unittest.main()
