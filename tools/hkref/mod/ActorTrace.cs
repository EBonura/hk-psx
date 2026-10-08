// Per-frame actor trace for the side-by-side harness: every active HealthManager
// near the hero (name, position, hp) and the active state of every PlayMaker FSM
// on it, plus the hero's own FSMs. Read-only, sampled in LateUpdate.
using System;
using System.Collections.Generic;
using System.Globalization;
using System.IO;
using System.Text;
using UnityEngine;

namespace HKReference
{
    public static class ActorTrace
    {
        private static StreamWriter writer;
        private static readonly List<HealthManager> actors = new List<HealthManager>();
        private static int captures;
        /// <summary>Sample every Nth frame (the survey tours thousands of frames).</summary>
        public static int Stride = 1;
        private const float Radius = 400f;

        public static void Initialize(string output)
        {
            writer = new StreamWriter(System.IO.Path.Combine(output, "actors.csv"), false);
            writer.WriteLine("test_frame,id,name,x,y,hp,dead,active,fsms,scene");
        }

        public static void Capture(int frame, Component hero)
        {
            if (writer == null || frame % Stride != 0) return;
            if (captures++ % 30 == 0) { actors.Clear(); actors.AddRange(UnityEngine.Object.FindObjectsOfType<HealthManager>(true)); }
            Vector3 h = hero == null ? Vector3.zero : hero.transform.position;
            foreach (HealthManager m in actors)
            {
                if (m == null || !m.gameObject.activeInHierarchy) continue;
                Vector3 p = m.transform.position;
                if (Mathf.Abs(p.x - h.x) > Radius || Mathf.Abs(p.y - h.y) > Radius) continue;
                StringBuilder fsms = new StringBuilder();
                foreach (PlayMakerFSM f in m.GetComponents<PlayMakerFSM>())
                    fsms.Append(f.FsmName).Append('=').Append(f.ActiveStateName).Append(';');
                writer.WriteLine(string.Join(",", new string[] {
                    frame.ToString(CultureInfo.InvariantCulture), m.GetInstanceID().ToString(CultureInfo.InvariantCulture),
                    "\"" + NameOf(m.transform) + "\"", p.x.ToString("F4", CultureInfo.InvariantCulture), p.y.ToString("F4", CultureInfo.InvariantCulture),
                    m.hp.ToString(CultureInfo.InvariantCulture), m.isDead ? "1" : "0", "1", "\"" + fsms.ToString() + "\"", m.gameObject.scene.name }));
            }
            if (captures % 60 == 0) writer.Flush();
        }

        private static string NameOf(Transform t) { return t.name; }
        public static void Dispose() { if (writer != null) { writer.Dispose(); writer = null; } }
    }
}
