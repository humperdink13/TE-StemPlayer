/**
 * sp1_board.h - SP-1 Stem Player board definition
 * Minimal board file - no LEDs/buttons used in bridge mode
 */

#ifndef SP1_BOARD_H
#define SP1_BOARD_H

#ifdef __cplusplus
extern "C" {
#endif

#include "nrf_gpio.h"

/* No LEDs used */
#define LEDS_NUMBER    0
#define LEDS_ACTIVE_STATE 0
#define LEDS_LIST {}
#define LEDS_INV_MASK  0

/* No buttons used */
#define BUTTONS_NUMBER 0
#define BUTTONS_ACTIVE_STATE 0
#define BUTTONS_LIST {}

/* BSP defines */
#define BSP_BOARD_LED_0 0
#define BSP_BOARD_LED_1 1
#define BSP_BOARD_LED_2 2
#define BSP_BOARD_LED_3 3

#ifdef __cplusplus
}
#endif

#endif /* SP1_BOARD_H */
