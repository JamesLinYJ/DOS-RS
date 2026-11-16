/* Exercise the hardware arithmetic fault path with a runtime integer divisor. */
static volatile unsigned divisor;
int main(void) { return 123U / divisor; }
