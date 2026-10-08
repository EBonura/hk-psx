// Explicit diagnostic setup only; fresh synthetic systems in the isolated native
// Unity player. Does not modify retail prefabs, scene objects, files or saves.
using System;
using System.Globalization;
using System.IO;
using UnityEngine;

namespace HKReference
{
    internal static class ParticleProbe
    {
        private static string F(float x) { return x.ToString("R", CultureInfo.InvariantCulture); }
        internal static void Run(string output)
        {
            using (StreamWriter log = new StreamWriter(Path.Combine(output, "particle-scale.csv"), false))
            {
                log.WriteLine("mode,scale,scale_y,scale_z,case,time,count,px,py,pz,vx,vy,vz,total_vx,total_vy,total_vz,size,baked_width,baked_height");
                foreach (ParticleSystemScalingMode mode in new[] { ParticleSystemScalingMode.Shape, ParticleSystemScalingMode.Hierarchy })
                foreach (Vector3 scale in new[] { Vector3.one, Vector3.one * 0.8120299577713013f, new Vector3(0.8122697472572327f, 0.8122696280479431f, 0.8120299577713013f) })
                foreach (string kind in new[] { "launch", "world_velocity", "world_force", "limit" })
                {
                    GameObject parent = new GameObject("HK Particle Scale Probe Parent");
                    GameObject host = new GameObject("HK Particle Scale Probe");
                    try
                    {
                        parent.transform.localScale = scale;
                        host.transform.SetParent(parent.transform, false);
                        ParticleSystem ps = host.AddComponent<ParticleSystem>();
                        ps.Stop(true, ParticleSystemStopBehavior.StopEmittingAndClear);
                        ps.useAutoRandomSeed = false; ps.randomSeed = 123;
                        ParticleSystem.MainModule main = ps.main;
                        main.playOnAwake = false; main.loop = false;
                        main.simulationSpace = ParticleSystemSimulationSpace.World;
                        main.scalingMode = mode; main.startLifetime = 10f;
                        main.startSpeed = (kind == "launch" || kind == "limit") ? 10f : 0f;
                        main.startSize = 0.6f; main.gravityModifier = 0f;
                        ParticleSystem.EmissionModule emission = ps.emission; emission.enabled = false;
                        ParticleSystem.ShapeModule shape = ps.shape;
                        shape.enabled = true; shape.shapeType = ParticleSystemShapeType.Circle;
                        shape.radius = 1f; shape.arc = 180f;
                        if (kind == "world_velocity")
                        {
                            ParticleSystem.VelocityOverLifetimeModule velocity = ps.velocityOverLifetime;
                            velocity.enabled = true; velocity.space = ParticleSystemSimulationSpace.World;
                            velocity.x = 0f; velocity.y = 5f; velocity.z = 0f;
                        }
                        if (kind == "world_force")
                        {
                            ParticleSystem.ForceOverLifetimeModule force = ps.forceOverLifetime;
                            force.enabled = true; force.space = ParticleSystemSimulationSpace.World;
                            force.x = 0f; force.y = -10f; force.z = 0f;
                        }
                        if (kind == "limit")
                        {
                            ParticleSystem.LimitVelocityOverLifetimeModule limit = ps.limitVelocityOverLifetime;
                            limit.enabled = true; limit.separateAxes = false; limit.limit = 3f; limit.dampen = 1f;
                        }
                        ps.Emit(1);
                        ParticleSystem.Particle[] particles = new ParticleSystem.Particle[4];
                        for (int step = 0; step < 2; step++)
                        {
                            if (step != 0) ps.Simulate(0.1f, false, false, false);
                            int count = ps.GetParticles(particles);
                            if (count != 1) throw new InvalidOperationException("Probe expected one native particle");
                            ParticleSystem.Particle p = particles[0]; Vector3 total = p.totalVelocity;
                            float width = -1f, height = -1f;
                            GameObject cameraHost = new GameObject("HK Particle Probe Camera");
                            Mesh mesh = new Mesh();
                            try
                            {
                                Camera camera = cameraHost.AddComponent<Camera>(); camera.enabled = false;
                                cameraHost.transform.position = p.position - Vector3.forward * 10f;
                                ParticleSystemRenderer renderer = ps.GetComponent<ParticleSystemRenderer>();
                                renderer.BakeMesh(mesh, camera, ParticleSystemBakeMeshOptions.BakePosition | ParticleSystemBakeMeshOptions.BakeRotationAndScale);
                                if (mesh.vertexCount != 4) throw new InvalidOperationException("Probe expected one baked particle quad");
                                mesh.RecalculateBounds(); width = mesh.bounds.size.x; height = mesh.bounds.size.y;
                            }
                            finally { UnityEngine.Object.DestroyImmediate(mesh); UnityEngine.Object.DestroyImmediate(cameraHost); }
                            log.WriteLine(mode + "," + F(scale.x) + "," + F(scale.y) + "," + F(scale.z) + "," + kind + "," + F(step * 0.1f) + "," + count + "," +
                                F(p.position.x) + "," + F(p.position.y) + "," + F(p.position.z) + "," +
                                F(p.velocity.x) + "," + F(p.velocity.y) + "," + F(p.velocity.z) + "," +
                                F(total.x) + "," + F(total.y) + "," + F(total.z) + "," + F(p.GetCurrentSize(ps)) + "," + F(width) + "," + F(height));
                        }
                    }
                    finally { UnityEngine.Object.DestroyImmediate(host); UnityEngine.Object.DestroyImmediate(parent); }
                }
            }
        }
    }
}
