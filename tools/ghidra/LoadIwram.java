// Ghidra pre-script: fill IWRAM (0x03000000) with a runtime dump so the ARM code the game copies there
// gets analysed at its real addresses, and mark every `stmfd sp!, {..., lr}` in it as a function start.
// Args: <iwram.bin>
import ghidra.app.script.GhidraScript;
import ghidra.program.model.address.Address;
import ghidra.program.model.mem.Memory;
import ghidra.program.model.mem.MemoryBlock;
import java.nio.file.Files;
import java.nio.file.Path;

public class LoadIwram extends GhidraScript {
    @Override
    public void run() throws Exception {
        byte[] data = Files.readAllBytes(Path.of(getScriptArgs()[0]));
        Address base = toAddr(0x03000000);
        Memory mem = currentProgram.getMemory();
        MemoryBlock b = mem.getBlock(base);
        if (b == null) {
            b = mem.createInitializedBlock("IWRAM", base, data.length, (byte) 0, monitor, false);
        } else if (!b.isInitialized()) {
            b = mem.convertToInitialized(b, (byte) 0);
        }
        mem.setBytes(base, data);
        b.setExecute(true);
        int n = 0;
        for (int off = 0; off + 4 <= data.length; off += 4) {
            int w = (data[off] & 0xff) | (data[off + 1] & 0xff) << 8 | (data[off + 2] & 0xff) << 16 | (data[off + 3] & 0xff) << 24;
            if ((w & 0xFFFF4000) == 0xE92D4000 && getFunctionAt(base.add(off)) == null) {
                disassemble(base.add(off));
                createFunction(base.add(off), null);
                n++;
            }
        }
        println("IWRAM loaded, " + n + " ARM function starts marked");
    }
}
