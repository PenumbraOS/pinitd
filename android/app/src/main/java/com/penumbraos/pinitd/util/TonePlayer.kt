package com.penumbraos.pinitd.util

import android.media.AudioAttributes
import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import android.util.Log
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin
import kotlin.math.sqrt
import kotlin.math.tanh

private const val SAMPLE_RATE = 44100

class TonePlayer {
    data class SoundEvent(
        /**
         * Empty array implies a rest. A single value implies a single note. Multiple values implies a chord
         */
        val frequenciesHz: DoubleArray,
        val durationMs: Int,
        val attackDurationMs: Int = 200,
        val releaseDurationMs: Int = 200,
        val waveform: Waveform = Waveform.SINE,
        val harmonics: List<Pair<Int, Double>> = emptyList(),
        val detuneHz: Double = 0.0,
        /**
         * Can be negative for overlap
         */
        val offsetMs: Int = 0
    )

    enum class Waveform {
        SINE, SQUARE, TRIANGLE, SAWTOOTH;

        fun sample(i: Int, freq: Double): Double {
            val t = i.toDouble() / SAMPLE_RATE // FIXED
            return when (this) {
                SINE -> sin(2.0 * PI * freq * t)
                SQUARE -> if (sin(2.0 * PI * freq * t) >= 0) 1.0 else -1.0
                TRIANGLE -> 2.0 * abs(2.0 * (freq * t % 1.0) - 1.0) - 1.0
                SAWTOOTH -> 2.0 * (freq * t % 1.0) - 1.0
            }.coerceIn(-1.0, 1.0) // Clamp output
        }
    }

    var currentTrack: AudioTrack? = null

    fun playJingle(events: List<SoundEvent>, volume: Float = 1.0f) {
        if (events.isEmpty()) return

        // Calculate total duration considering offsets
        var currentTimeMs = 0
        var maxEndMs = 0

        // startSample, event
        val scheduledEvents = mutableListOf<Pair<Int, SoundEvent>>()

        for (event in events) {
            val startMs = currentTimeMs + event.offsetMs
            val endMs = startMs + event.durationMs

            val startSample = (startMs / 1000.0 * SAMPLE_RATE).toInt().coerceAtLeast(0)
            scheduledEvents.add(startSample to event)

            if (endMs > maxEndMs) maxEndMs = endMs
            currentTimeMs = endMs
        }

        val totalSamples = (maxEndMs / 1000.0 * SAMPLE_RATE).toInt().coerceAtLeast(1)
        val mixBuffer = DoubleArray(totalSamples)

        for ((startSample, event) in scheduledEvents) {
            val samples = (event.durationMs / 1000.0 * SAMPLE_RATE).toInt()
            val attackSamples = (event.attackDurationMs / 1000.0 * SAMPLE_RATE).toInt()
            val releaseSamples = (event.releaseDurationMs / 1000.0 * SAMPLE_RATE).toInt()

            require(event.attackDurationMs + event.releaseDurationMs <= event.durationMs) {
                "Attack and release durations combined must be <= event duration"
            }

            for (i in 0 until samples) {
                var sampleSum = 0.0
                if (event.frequenciesHz.isNotEmpty()) {
                    for (freq in event.frequenciesHz) {
                        val baseFreq = freq + if (event.detuneHz != 0.0) {
                            if ((freq.hashCode() and 1) == 0) event.detuneHz else -event.detuneHz
                        } else 0.0

                        sampleSum += event.waveform.sample(i, baseFreq)

                        for ((multiple, amp) in event.harmonics) {
                            sampleSum += amp * event.waveform.sample(i, baseFreq * multiple)
                        }
                    }
                    sampleSum /= event.frequenciesHz.size
                }

                val amplitude = when {
                    i < attackSamples -> i.toDouble() / attackSamples
                    i > samples - releaseSamples -> (samples - i).toDouble() / releaseSamples
                    else -> 1.0
                }

                val sampleValue = (sampleSum * amplitude).coerceIn(-1.0, 1.0)
                val idx = startSample + i
                if (idx < mixBuffer.size) {
                    mixBuffer[idx] += sampleValue
                }
            }
        }

        // Normalize to prevent clipping and apply volume
//        val maxAmp = mixBuffer.maxOf { abs(it) }.coerceAtLeast(1e-6)
//        val pcm = ShortArray(totalSamples) { i ->
//            (mixBuffer[i] / maxAmp * Short.MAX_VALUE).toInt().toShort()
//        }
        // Measure peak and RMS after mixing
        val peak = mixBuffer.maxOf { abs(it) }.coerceAtLeast(1e-12)
        val rms  = sqrt(mixBuffer.sumOf { it * it } / mixBuffer.size)

        // Compute master gain: only scale down if we exceeded 1.0
        // Keep some headroom to avoid intersample clipping (e.g., 0.8)
        val headroom = 0.8
        val normalizedVolume = volume.coerceIn(0f, 1f).toDouble()
        val downscale = if (peak > 1.0) (headroom / peak) else headroom
        val masterGain = normalizedVolume * downscale

        Log.e("TonePlayer",
            "post-mix peak=%.3f, rms=%.3f, masterGain=%.3f (vol=%.2f)"
                .format(peak, rms, masterGain, volume))

        fun softClip(x: Double): Double {
            // gentle: y = tanh(2x)/tanh(2) keeps |y|<1 with soft knee
            val y = tanh(2.0 * x)
            val n = tanh(2.0)
            return y / n
        }

        val pcm = ShortArray(mixBuffer.size) { i ->
            val s = softClip(mixBuffer[i] * masterGain)
                .coerceIn(-1.0, 1.0)
            (s * Short.MAX_VALUE).toInt().toShort()
        }

        val attrs = AudioAttributes.Builder()
            .setUsage(AudioAttributes.USAGE_ASSISTANCE_SONIFICATION) // try USAGE_NOTIFICATION as well
            .setContentType(AudioAttributes.CONTENT_TYPE_SONIFICATION)
            .build()

        val format = AudioFormat.Builder()
            .setSampleRate(SAMPLE_RATE)
            .setEncoding(AudioFormat.ENCODING_PCM_16BIT)
            .setChannelMask(AudioFormat.CHANNEL_OUT_MONO)
            .build()

        val track = AudioTrack(attrs, format, pcm.size * 2, AudioTrack.MODE_STATIC, AudioManager.AUDIO_SESSION_ID_GENERATE)

        currentTrack = track
        track.write(pcm, 0, pcm.size)
        track.play()
    }
}
