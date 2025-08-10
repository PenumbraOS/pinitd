package com.penumbraos.pinitd.util

import android.media.AudioFormat
import android.media.AudioManager
import android.media.AudioTrack
import kotlin.math.PI
import kotlin.math.abs
import kotlin.math.sin

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
        val detuneHz: Double = 0.0
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

    fun playJingle(
        events: List<SoundEvent>,
    ) {
        val buffer = mutableListOf<Short>()

        for (event in events) {
            val samples = (event.durationMs / 1000.0 * SAMPLE_RATE).toInt()
            val attackSamples = (event.attackDurationMs / 1000.0 * SAMPLE_RATE).toInt()
            val releaseSamples = (event.releaseDurationMs / 1000.0 * SAMPLE_RATE).toInt()

            require(event.attackDurationMs + event.releaseDurationMs <= event.durationMs) {
                "Attack and release durations combined must be less than the event duration"
            }

            for (i in 0 until samples) {
                var sampleSum = 0.0
                if (event.frequenciesHz.isNotEmpty()) {
                    for (freq in event.frequenciesHz) {
                        val baseFreq = freq + if (event.detuneHz != 0.0) {
                            if ((freq.hashCode() and 1) == 0) event.detuneHz else -event.detuneHz
                        } else 0.0

                        // Base tone
                        sampleSum += event.waveform.sample(i, baseFreq)

                        // Harmonics
                        for ((multiple, amp) in event.harmonics) {
                            sampleSum += amp * event.waveform.sample(i, baseFreq * multiple)
                        }
                    }
                    sampleSum /= event.frequenciesHz.size
                }

                // Envelope
                val amplitude = when {
                    i < attackSamples -> i.toDouble() / attackSamples
                    i > samples - releaseSamples ->
                        (samples - i).toDouble() / releaseSamples
                    else -> 1.0
                }

                // Clamp to [-1, 1] before converting to Short
                val sample = (sampleSum * amplitude).coerceIn(-1.0, 1.0)
                buffer.add((sample * Short.MAX_VALUE).toInt().toShort())
            }
        }

//        while (buffer.size < SAMPLE_RATE) {
//            buffer.add(0)
//        }

        val track = AudioTrack(
            AudioManager.STREAM_MUSIC,
            SAMPLE_RATE,
            AudioFormat.CHANNEL_OUT_MONO,
            AudioFormat.ENCODING_PCM_16BIT,
            buffer.size * 2,
            AudioTrack.MODE_STATIC
        )

        currentTrack = track

        val pcm = buffer.toShortArray()
        track.write(pcm, 0, pcm.size)
        track.play()
    }
}
