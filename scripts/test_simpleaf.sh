#!/bin/bash
# In this script we test simpleaf using a toy read-reference set
# template took from here https://stackoverflow.com/a/34676160/18156398
# the directory of the script
DIR="$( cd "$( dirname "${BASH_SOURCE[0]}" )" && pwd )"
SIMPLEAF=${DIR}/../target/release/simpleaf
echo "Testing simpleaf using a toy read-reference set"

# the temp directory used, within $DIR
# omit the -p parameter to create a temporal directory in the default location
#WORK_DIR=`mktemp -d -p "$DIR"`
WORK_DIR=`mktemp -d`
LOG_DIR="${WORK_DIR}/simpleaf_logs"
ALEVIN_FRY_HOME="${WORK_DIR}/alevin_fry_home"
mkdir -p $ALEVIN_FRY_HOME
mkdir -p $LOG_DIR

# check if tmp dir was created
if [[ ! "$WORK_DIR" || ! -d "$WORK_DIR" ]]; then
        echo "Could not create temp dir"
        exit 1
fi

# deletes the temp directory
function cleanup {      
        rm -rf "$WORK_DIR"
        echo "  - Deleted temp working directory $WORK_DIR"
}

# implementation of script starts here
echo "  - Downloading the toy read-reference set"
wget https://umd.box.com/shared/static/lx2xownlrhz3us8496tyu9c4dgade814.gz  -O  ${WORK_DIR}/toy_read_ref_set.tar.gz -q
tar -xf ${WORK_DIR}/toy_read_ref_set.tar.gz -C ${WORK_DIR}

echo "  - Testing simpleaf set-paths"
set_paths_cmd="ALEVIN_FRY_HOME=$ALEVIN_FRY_HOME \
${SIMPLEAF} set-paths"
eval $set_paths_cmd
status=$?

#echo "  - Setting custom chemistry"
#add_chem_cmd="ALEVIN_FRY_HOME=$ALEVIN_FRY_HOME \
#${SIMPLEAF} add-chemistry --name flarb_flub --geometry "\""B1[1-16];U1[17-28];R2[1-end]"\"
#echo $add_chem_cmd
#eval $add_chem_cmd
#status=$?

if [ $status -ne 0 ]; then
        echo "ERROR when running simpleaf set-paths"
        exit 1
else
        echo "simpleaf set-paths ran successfully"
fi

echo "  - Testing simpleaf index"
REF_DIR="${WORK_DIR}/toy_human_ref"
index_cmd="ALEVIN_FRY_HOME=$ALEVIN_FRY_HOME \
${SIMPLEAF} index --ref-type spliced+intronic -f ${REF_DIR}/fasta/genome.fa \
-g ${REF_DIR}/genes/genes.gtf \
-r 91 -o ${WORK_DIR}/test_index_outdir"
eval $index_cmd
status=$?

if [ $status -ne 0 ]; then
        echo "ERROR when running simpleaf index"
        exit 1
else
        echo "simpleaf index ran successfully"
fi

echo "  - Testing simpleaf quant"
FASTQ_DIR="${WORK_DIR}/toy_read_fastq"
quant_cmd="ALEVIN_FRY_HOME=$ALEVIN_FRY_HOME \
${SIMPLEAF} quant \
-1 ${FASTQ_DIR}/selected_R1_reads.fastq \
-2 ${FASTQ_DIR}/selected_R2_reads.fastq \
-i ${WORK_DIR}/test_index_outdir/index \
-o ${WORK_DIR}/test_quant_outdir \
-u -c 10xv3 -r cr-like \
-m ${WORK_DIR}/test_index_outdir/index/t2g_3col.tsv \
-t 16"
eval $quant_cmd
status=$?

if [ $status -ne 0 ]; then
        echo "ERROR when running simpleaf quant"
        exit 1
else
        echo "  - simpleaf quant ran successfully"
fi
# register the cleanup function to be called on the EXIT signal
status=$?
#[ "$status" -eq 0 ] && rm -rf $WORK_DIR 

echo "simpleaf works!"
