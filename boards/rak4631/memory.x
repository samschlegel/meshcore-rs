/* RAK4631 nRF52840 memory layout — Adafruit UF2 bootloader without SoftDevice */
/* MBR/bootloader occupies 0x00000000-0x00001000; app starts at 0x1000 */
/* Last 4K page (0xFF000) reserved for NVM storage (RTC epoch persistence) */
MEMORY
{
  FLASH : ORIGIN = 0x00001000, LENGTH = 1016K
  RAM   : ORIGIN = 0x20000008, LENGTH = 255K
}
