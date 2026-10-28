import importlib.util
from pathlib import Path
import unittest
spec = importlib.util.spec_from_file_location("audio_evaluation", Path(__file__).parents[1] / "evaluate-audio-retrieval.py")
module = importlib.util.module_from_spec(spec)
spec.loader.exec_module(module)


class EvaluationTests(unittest.TestCase):
    def test_missing_labels_remain_unmeasured(self):
        report = module.evaluate({"queries": [{"query": "下雨", "relevant_asset_ids": []}]}, {})
        self.assertEqual(report["status"], "not_run_missing_labels")
        self.assertIsNone(report["metrics"]["top_10_hit_rate"])

    def test_real_rankings_score_only_labeled_queries(self):
        dataset = {"queries": [{"query": "rain", "relevant_asset_ids": ["a"]},
                               {"query": "door", "relevant_asset_ids": ["b"]},
                               {"query": "unlabeled", "relevant_asset_ids": []}]}
        report = module.evaluate(dataset, {"basis": "audio_content", "queries": {"rain": ["x", "x", "a"], "door": []}})
        self.assertEqual(report["status"], "evaluated")
        self.assertEqual(report["metrics"]["top_10_hit_rate"], .5)
        self.assertEqual(report["metrics"]["mean_reciprocal_rank"], .25)
        self.assertEqual(report["unlabeled_queries"], 1)

    def test_metadata_or_missing_runs_do_not_become_audio_quality(self):
        dataset = {"queries": [{"query": "rain", "relevant_asset_ids": ["a"]}]}
        with self.assertRaises(ValueError):
            module.evaluate(dataset, {"basis": "metadata_text", "queries": {"rain": ["a"]}})
        report = module.evaluate(dataset, {"basis": "audio_content", "queries": {}})
        self.assertEqual(report["status"], "incomplete_results")
        self.assertIsNone(report["metrics"]["mean_reciprocal_rank"])


if __name__ == "__main__":
    unittest.main()
