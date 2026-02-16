package com.penumbraos.pinitd.util

fun playBootChime(player: TonePlayer, volume: Float = 1.0f) {
    // Gentle rising two-tone chime (inspired by iOS charging sound)
    // Two pure sine tones ascending a major sixth — soft and brief
    player.playJingle(
        listOf(
            TonePlayer.SoundEvent(
                doubleArrayOf(587.33),  // D5
                durationMs = 180,
                attackDurationMs = 15,
                releaseDurationMs = 100,
                waveform = TonePlayer.Waveform.SINE
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(880.00),  // A5
                durationMs = 300,
                attackDurationMs = 15,
                releaseDurationMs = 250,
                waveform = TonePlayer.Waveform.SINE,
                offsetMs = -30
            )
        ),
        volume
    )
}

fun playDeathChime(player: TonePlayer, volume: Float = 1.0f) {
    // Gentle descending two-tone chime — soft and unmistakably different from boot
    player.playJingle(
        listOf(
            TonePlayer.SoundEvent(
                doubleArrayOf(659.25),  // E5
                durationMs = 250,
                attackDurationMs = 15,
                releaseDurationMs = 150,
                waveform = TonePlayer.Waveform.SINE
            ),
            TonePlayer.SoundEvent(
                doubleArrayOf(440.00),  // A4
                durationMs = 400,
                attackDurationMs = 15,
                releaseDurationMs = 350,
                waveform = TonePlayer.Waveform.SINE,
                offsetMs = -30
            )
        ),
        volume
    )
}
