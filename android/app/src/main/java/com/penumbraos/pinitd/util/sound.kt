package com.penumbraos.pinitd.util

fun playBootChime(player: TonePlayer, volume: Float = 1.0f) {
    // Gentle rising major fifth chime (inspired by iOS charging sound)
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
    // Gentle descending minor third chime
    player.playJingle(
        listOf(
            TonePlayer.SoundEvent(
                doubleArrayOf(523.25),  // C5
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
