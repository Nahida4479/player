# Instrumentation runs parent-first against the separately shrunk host.
-keep class androidx.tracing.Trace { *; }
-keep class dev.verncat.player.InstalledApkRuntime { *; }
-keep class dev.verncat.player.InstalledApkRuntime$Descriptor { *; }
-keep class dev.verncat.player.PluginClassLoader { *; }
-keep class kotlin.jvm.internal.Intrinsics { *; }
-keep interface kotlin.jvm.functions.Function3 { *; }
